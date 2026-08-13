import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import path from "node:path";
import test from "node:test";

import {
  ENCODED_FLAG_SEPARATOR,
  LAUNCHER_ERROR_MESSAGE,
  buildRemappedEnvironment,
  requiresRemapping,
  resolveTauriCliScript,
  runTauri,
  slashVariants,
  splitEffectiveRustflags,
} from "../run-tauri-remapped.mjs";

const WINDOWS_PROJECT = String.raw`C:\Users\Build User\skill deck`;
const FIXTURE_HOME = path.join(
  path.parse(process.cwd()).root,
  "skill-deck-fixture",
  "Build User",
);
const FIXTURE_PROJECT = path.join(FIXTURE_HOME, "skill deck");
const FIXTURE_SYSROOT = path.join(
  FIXTURE_HOME,
  ".rustup",
  "toolchains",
  "stable",
);

function encodedFlags(env) {
  return env.CARGO_ENCODED_RUSTFLAGS.split(ENCODED_FLAG_SEPARATOR);
}

test("build and bundle remap while dev passes through", () => {
  assert.equal(requiresRemapping(["build", "--no-bundle"]), true);
  assert.equal(requiresRemapping(["bundle"]), true);
  assert.equal(requiresRemapping(["dev"]), false);
  assert.equal(requiresRemapping(["info"]), false);
});

test("slash variants preserve both Windows path separators", () => {
  assert.deepEqual(slashVariants(WINDOWS_PROJECT), [
    WINDOWS_PROJECT.replaceAll("\\", "/"),
    WINDOWS_PROJECT,
  ]);
});

test("encoded flags are preserved and remaps handle spaces and slash variants", () => {
  const existing = [
    "-Copt-level=2",
    "--cfg=existing_flag",
  ].join(ENCODED_FLAG_SEPARATOR);
  const env = buildRemappedEnvironment({
    baseEnv: {
      CARGO_ENCODED_RUSTFLAGS: existing,
      RUSTFLAGS: '"unterminated',
      CARGO_HOME: path.join(FIXTURE_HOME, ".cargo"),
      RUSTUP_HOME: path.join(FIXTURE_HOME, ".rustup"),
      CARGO_TARGET_DIR: path.join(FIXTURE_PROJECT, "src-tauri", "target"),
    },
    projectRoot: FIXTURE_PROJECT,
    userHome: FIXTURE_HOME,
    rustcSysroot: FIXTURE_SYSROOT,
  });
  const flags = encodedFlags(env);
  const fixtureTarget = path.join(FIXTURE_PROJECT, "src-tauri", "target");
  assert.deepEqual(flags.slice(0, 2), [
    "-Copt-level=2",
    "--cfg=existing_flag",
  ]);
  assert.equal("RUSTFLAGS" in env, false);

  const remaps = flags.slice(2);
  assert.ok(
    remaps.includes(
      `--remap-path-prefix=${FIXTURE_PROJECT}=/skill-deck/source`,
    ),
  );
  assert.ok(
    remaps.includes(
      `--remap-path-prefix=${FIXTURE_PROJECT.replaceAll("\\", "/")}=/skill-deck/source`,
    ),
  );
  assert.ok(
    remaps.some(
      (flag) =>
        flag ===
        `--remap-path-prefix=${fixtureTarget}=/skill-deck/target`,
    ),
  );
  assert.equal(new Set(remaps).size, remaps.length);

  const sourceLengths = remaps.map((flag) => {
    const mapping = flag.slice("--remap-path-prefix=".length);
    return mapping.slice(0, mapping.lastIndexOf("=")).length;
  });
  assert.deepEqual(sourceLengths, [...sourceLengths].sort((a, b) => a - b));
  assert.ok(
    remaps.indexOf(
      `--remap-path-prefix=${FIXTURE_PROJECT}=/skill-deck/source`,
    ) <
      remaps.indexOf(
        `--remap-path-prefix=${fixtureTarget}=/skill-deck/target`,
      ),
  );
  assert.ok(
    remaps.indexOf(
      `--remap-path-prefix=${path.join(FIXTURE_HOME, ".rustup")}=/rust/rustup-home`,
    ) <
      remaps.indexOf(
        `--remap-path-prefix=${FIXTURE_SYSROOT}=/rust/sysroot`,
      ),
  );
});

test("RUSTFLAGS parser preserves quotes, escaped whitespace, and backslashes", () => {
  assert.deepEqual(
    splitEffectiveRustflags(
      String.raw`-C "link-arg=-LIBPATH:C:\Program Files\Rust" --cfg 'feature="quoted value"' C:\plain\path escaped\ whitespace backslash\\value ""`,
    ),
    [
      "-C",
      String.raw`link-arg=-LIBPATH:C:\Program Files\Rust`,
      "--cfg",
      'feature="quoted value"',
      String.raw`C:\plain\path`,
      "escaped whitespace",
      String.raw`backslash\value`,
      "",
    ],
  );
});

test("ordinary RUSTFLAGS are encoded deterministically", () => {
  const options = {
    baseEnv: {
      RUSTFLAGS:
        String.raw`-C "link-arg=-LIBPATH:C:\Program Files\Rust" --cfg 'release fixture'`,
    },
    projectRoot: path.resolve("fixture-project"),
    userHome: path.resolve("fixture-home"),
    rustcSysroot: path.resolve("fixture-sysroot"),
  };
  const env = buildRemappedEnvironment(options);
  const repeated = buildRemappedEnvironment(options);
  assert.equal(
    env.CARGO_ENCODED_RUSTFLAGS,
    repeated.CARGO_ENCODED_RUSTFLAGS,
  );
  assert.deepEqual(encodedFlags(env).slice(0, 4), [
    "-C",
    String.raw`link-arg=-LIBPATH:C:\Program Files\Rust`,
    "--cfg",
    "release fixture",
  ]);
  assert.equal("RUSTFLAGS" in env, false);
});

test("malformed or unbounded ordinary RUSTFLAGS fail closed", () => {
  for (const malformed of [
    '"unterminated',
    "'unterminated",
    "dangling\\",
  ]) {
    assert.throws(() => splitEffectiveRustflags(malformed), /RUSTFLAGS/u);
  }
  assert.throws(
    () => splitEffectiveRustflags("a b c", { maxArguments: 2 }),
    /too many/u,
  );
  assert.throws(
    () => splitEffectiveRustflags("oversized", { maxLength: 4 }),
    /parser limit/u,
  );
});

test("effective simple RUSTFLAGS remain deliberately encoded", () => {
  const env = buildRemappedEnvironment({
    baseEnv: {
      RUSTFLAGS: "  -C debuginfo=0 --cfg release_fixture  ",
    },
    projectRoot: path.resolve("fixture-project"),
    userHome: path.resolve("fixture-home"),
    rustcSysroot: path.resolve("fixture-sysroot"),
  });
  assert.deepEqual(encodedFlags(env).slice(0, 4), [
    "-C",
    "debuginfo=0",
    "--cfg",
    "release_fixture",
  ]);
  assert.equal("RUSTFLAGS" in env, false);
});

test("dev invokes the resolved local CLI through Node without environment changes", async () => {
  const calls = [];
  const child = new EventEmitter();
  const env = { SENTINEL: "unchanged", RUSTFLAGS: "-C debuginfo=2" };
  const nodeExecutable = path.resolve("fixture-node.exe");
  const tauriCliScript = path.resolve("fixture-tauri.js");
  const resultPromise = runTauri(["dev", "--no-watch"], {
    env,
    cwd: path.resolve("fixture-project"),
    nodeExecutable,
    resolveTauriCliScriptImpl() {
      return tauriCliScript;
    },
    spawnSyncImpl() {
      throw new Error("rustc must not run for dev");
    },
    spawnImpl(command, args, options) {
      calls.push({ command, args, options });
      queueMicrotask(() => child.emit("close", 23, null));
      return child;
    },
  });
  assert.deepEqual(await resultPromise, { code: 23, signal: null });
  assert.equal(calls[0].command, nodeExecutable);
  assert.deepEqual(calls[0].args, [tauriCliScript, "dev", "--no-watch"]);
  assert.equal(calls[0].options.env, env);
  assert.equal(calls[0].options.shell, false);
  assert.equal(calls[0].command.toLowerCase().endsWith(".cmd"), false);
});

test("local Tauri CLI resolution is absolute and fails closed", () => {
  const expected = path.resolve("node_modules", "@tauri-apps", "cli", "tauri.js");
  assert.equal(resolveTauriCliScript(() => expected), expected);
  assert.throws(
    () => resolveTauriCliScript(() => "relative-tauri.js"),
    /resolution failed/u,
  );
});

test("child signals and spawn errors are preserved without source-prefix output", async () => {
  for (const outcome of [
    { event: "close", values: [null, "SIGTERM"], expected: { code: 1, signal: "SIGTERM" } },
    { event: "error", values: [new Error(WINDOWS_PROJECT)], expected: { code: 1, signal: null } },
  ]) {
    const child = new EventEmitter();
    const promise = runTauri(["dev"], {
      resolveTauriCliScriptImpl() {
        return path.resolve("fixture-tauri.js");
      },
      spawnImpl() {
        queueMicrotask(() => child.emit(outcome.event, ...outcome.values));
        return child;
      },
    });
    assert.deepEqual(await promise, outcome.expected);
  }
  assert.equal(LAUNCHER_ERROR_MESSAGE.includes(WINDOWS_PROJECT), false);
});
