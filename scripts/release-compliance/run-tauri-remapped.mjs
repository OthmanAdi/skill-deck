#!/usr/bin/env node

import { homedir } from "node:os";
import { createRequire } from "node:module";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";

export const ENCODED_FLAG_SEPARATOR = "\u001f";
export const LAUNCHER_ERROR_MESSAGE =
  "tauri-remap: ERROR: remapped build environment unavailable";
export const MAX_RUSTFLAGS_LENGTH = 64 * 1024;
export const MAX_RUSTFLAGS_ARGUMENTS = 4096;

const moduleRequire = createRequire(import.meta.url);

const NEUTRAL_PREFIXES = Object.freeze({
  target: "/skill-deck/target",
  sysroot: "/rust/sysroot",
  cargoHome: "/rust/cargo-home",
  rustupHome: "/rust/rustup-home",
  project: "/skill-deck/source",
});

function compareText(left, right) {
  return left < right ? -1 : left > right ? 1 : 0;
}

function resolveFromProject(projectRoot, value) {
  return path.isAbsolute(value) ? path.normalize(value) : path.resolve(projectRoot, value);
}

export function slashVariants(source) {
  const normalized = path.normalize(source);
  const variants = new Set([normalized]);
  if (/^[A-Za-z]:[\\/]/.test(normalized) || normalized.includes("\\")) {
    variants.add(normalized.replaceAll("\\", "/"));
    variants.add(normalized.replaceAll("/", "\\"));
  }
  return [...variants].sort(compareText);
}

export function splitEffectiveRustflags(
  value,
  {
    maxLength = MAX_RUSTFLAGS_LENGTH,
    maxArguments = MAX_RUSTFLAGS_ARGUMENTS,
  } = {},
) {
  const input = String(value ?? "");
  if (input.length > maxLength) {
    throw new Error("RUSTFLAGS exceeds the parser limit");
  }

  const args = [];
  let current = "";
  let quote = null;
  let started = false;
  const pushCurrent = () => {
    if (!started) {
      return;
    }
    if (args.length >= maxArguments) {
      throw new Error("RUSTFLAGS contains too many arguments");
    }
    args.push(current);
    current = "";
    started = false;
  };

  for (let index = 0; index < input.length; index += 1) {
    const character = input[index];
    if (quote === "'") {
      if (character === "'") {
        quote = null;
      } else {
        current += character;
      }
      continue;
    }
    if (quote === '"') {
      if (character === '"') {
        quote = null;
        continue;
      }
      if (character === "\\") {
        if (index + 1 >= input.length) {
          throw new Error("RUSTFLAGS ends with an incomplete escape");
        }
        const next = input[index + 1];
        if (next === '"' || next === "\\" || /\s/u.test(next)) {
          current += next;
          index += 1;
        } else {
          current += character;
        }
        continue;
      }
      current += character;
      continue;
    }
    if (/\s/u.test(character)) {
      pushCurrent();
      continue;
    }
    if (character === "'" || character === '"') {
      quote = character;
      started = true;
      continue;
    }
    if (character === "\\") {
      if (index + 1 >= input.length) {
        throw new Error("RUSTFLAGS ends with an incomplete escape");
      }
      const next = input[index + 1];
      if (
        next === "'" ||
        next === '"' ||
        next === "\\" ||
        /\s/u.test(next)
      ) {
        current += next;
        index += 1;
      } else {
        current += character;
      }
      started = true;
      continue;
    }
    current += character;
    started = true;
  }
  if (quote !== null) {
    throw new Error("RUSTFLAGS contains an unterminated quote");
  }
  pushCurrent();
  return args;
}

export function buildRemapFlags({
  projectRoot,
  cargoHome,
  rustupHome,
  rustcSysroot,
  targetRoot,
}) {
  const roots = [
    { source: targetRoot, neutral: NEUTRAL_PREFIXES.target, rank: 0 },
    { source: rustcSysroot, neutral: NEUTRAL_PREFIXES.sysroot, rank: 1 },
    { source: cargoHome, neutral: NEUTRAL_PREFIXES.cargoHome, rank: 2 },
    { source: rustupHome, neutral: NEUTRAL_PREFIXES.rustupHome, rank: 3 },
    { source: projectRoot, neutral: NEUTRAL_PREFIXES.project, rank: 4 },
  ];
  const entries = [];
  const seen = new Set();
  for (const root of roots) {
    for (const source of slashVariants(root.source)) {
      const identity = /^[A-Za-z]:[\\/]/.test(source)
        ? source.toLowerCase()
        : source;
      if (seen.has(identity)) {
        continue;
      }
      seen.add(identity);
      entries.push({ source, neutral: root.neutral, rank: root.rank });
    }
  }
  entries.sort(
    (left, right) =>
      left.source.length - right.source.length ||
      compareText(left.source, right.source) ||
      left.rank - right.rank ||
      compareText(left.neutral, right.neutral),
  );
  return entries.map(
    ({ source, neutral }) => `--remap-path-prefix=${source}=${neutral}`,
  );
}

export function buildRemappedEnvironment({
  baseEnv = process.env,
  projectRoot = process.cwd(),
  userHome = homedir(),
  rustcSysroot,
} = {}) {
  if (!rustcSysroot) {
    throw new Error("rustc sysroot discovery failed");
  }
  const resolvedProject = path.resolve(projectRoot);
  const cargoHome = resolveFromProject(
    resolvedProject,
    baseEnv.CARGO_HOME || path.join(userHome, ".cargo"),
  );
  const rustupHome = resolveFromProject(
    resolvedProject,
    baseEnv.RUSTUP_HOME || path.join(userHome, ".rustup"),
  );
  const targetRoot = resolveFromProject(
    resolvedProject,
    baseEnv.CARGO_TARGET_DIR || path.join("src-tauri", "target"),
  );
  const remapFlags = buildRemapFlags({
    projectRoot: resolvedProject,
    cargoHome,
    rustupHome,
    rustcSysroot: path.normalize(rustcSysroot),
    targetRoot,
  });

  const childEnv = { ...baseEnv };
  const encoded = childEnv.CARGO_ENCODED_RUSTFLAGS;
  let prefix = "";
  if (typeof encoded === "string" && encoded.length > 0) {
    prefix = `${encoded}${ENCODED_FLAG_SEPARATOR}`;
  } else {
    const effectiveRustflags = splitEffectiveRustflags(childEnv.RUSTFLAGS);
    if (effectiveRustflags.length > 0) {
      prefix = `${effectiveRustflags.join(ENCODED_FLAG_SEPARATOR)}${ENCODED_FLAG_SEPARATOR}`;
    }
  }
  childEnv.CARGO_ENCODED_RUSTFLAGS = `${prefix}${remapFlags.join(
    ENCODED_FLAG_SEPARATOR,
  )}`;
  delete childEnv.RUSTFLAGS;
  return childEnv;
}

export function requiresRemapping(args) {
  return args[0] === "build" || args[0] === "bundle";
}

export function discoverRustcSysroot({
  env = process.env,
  cwd = process.cwd(),
  spawnSyncImpl = spawnSync,
} = {}) {
  const result = spawnSyncImpl("rustc", ["--print", "sysroot"], {
    cwd,
    env,
    encoding: "utf8",
    shell: false,
    stdio: ["ignore", "pipe", "ignore"],
    windowsHide: true,
  });
  const value =
    result && result.status === 0 && typeof result.stdout === "string"
      ? result.stdout.trim()
      : "";
  if (!value) {
    throw new Error("rustc sysroot discovery failed");
  }
  return value;
}

export function resolveTauriCliScript(
  resolveImpl = moduleRequire.resolve.bind(moduleRequire),
) {
  const resolved = resolveImpl("@tauri-apps/cli/tauri.js");
  if (typeof resolved !== "string" || !path.isAbsolute(resolved)) {
    throw new Error("local Tauri CLI resolution failed");
  }
  return resolved;
}

export function runTauri(
  args,
  {
    env = process.env,
    cwd = process.cwd(),
    nodeExecutable = process.execPath,
    resolveTauriCliScriptImpl = resolveTauriCliScript,
    spawnImpl = spawn,
    spawnSyncImpl = spawnSync,
  } = {},
) {
  let childEnv = env;
  if (requiresRemapping(args)) {
    childEnv = buildRemappedEnvironment({
      baseEnv: env,
      projectRoot: cwd,
      rustcSysroot: discoverRustcSysroot({
        env,
        cwd,
        spawnSyncImpl,
      }),
    });
  }

  const tauriCliScript = resolveTauriCliScriptImpl();
  return new Promise((resolve) => {
    const child = spawnImpl(nodeExecutable, [tauriCliScript, ...args], {
      cwd,
      env: childEnv,
      shell: false,
      stdio: "inherit",
      windowsHide: true,
    });
    let settled = false;
    child.once("error", () => {
      if (!settled) {
        settled = true;
        resolve({ code: 1, signal: null });
      }
    });
    child.once("close", (code, signal) => {
      if (!settled) {
        settled = true;
        resolve({ code: code ?? 1, signal: signal ?? null });
      }
    });
  });
}

export async function main(args = process.argv.slice(2)) {
  try {
    const result = await runTauri(args);
    if (result.signal) {
      try {
        process.kill(process.pid, result.signal);
        return;
      } catch {
        process.exitCode = 1;
        return;
      }
    }
    process.exitCode = result.code;
  } catch {
    console.error(LAUNCHER_ERROR_MESSAGE);
    process.exitCode = 1;
  }
}

if (
  process.argv[1] &&
  pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url
) {
  await main();
}
