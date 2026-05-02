<!--
  @agent-context: Smooth graph explorer for skill relationships.
  Uses a tiny deterministic force layout, no dependency, labels hidden by default.
-->
<script lang="ts">
  import { onDestroy } from "svelte";
  import type { Skill } from "$lib/types";
  import { DEFAULT_AGENT_COLOR } from "$lib/types";
  import { copySkillReference, store } from "$lib/stores/skills.svelte";

  type GraphNode = {
    id: string;
    skill: Skill;
    x: number;
    y: number;
    vx: number;
    vy: number;
    radius: number;
    degree: number;
    color: string;
  };

  type GraphEdge = {
    from: string;
    to: string;
  };

  const VIEWPORT_W = 1200;
  const VIEWPORT_H = 740;
  const CENTER_X = VIEWPORT_W / 2;
  const CENTER_Y = VIEWPORT_H / 2;
  const MIN_ZOOM = 0.45;
  const MAX_ZOOM = 2.8;
  const MAX_PAIRWISE_NODES = 140;
  const MAX_LAYOUT_TICKS = 56;

  let { skills }: { skills: Skill[] } = $props();

  let svgEl: SVGSVGElement | undefined = $state();
  let nodes = $state<GraphNode[]>([]);
  let edges = $state<GraphEdge[]>([]);
  let selectedSkillId = $state<string | null>(null);
  let hoveredSkillId = $state<string | null>(null);
  let labelsEnabled = $state(false);
  let isPanning = $state(false);

  let zoom = $state(1);
  let panX = $state(0);
  let panY = $state(0);
  let targetZoom = 1;
  let targetPanX = 0;
  let targetPanY = 0;

  let pointerId: number | null = null;
  let panStartClientX = 0;
  let panStartClientY = 0;
  let panStartX = 0;
  let panStartY = 0;
  let panMoved = false;

  let layoutRaf: number | null = null;
  let viewRaf: number | null = null;
  let fitRaf: number | null = null;
  const positionCache = new Map<string, { x: number; y: number }>();

  const reducedMotion = typeof window !== "undefined"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  const idToNode = $derived.by(() => {
    const map = new Map<string, GraphNode>();
    for (const node of nodes) map.set(node.id, node);
    return map;
  });

  const selectedSkill = $derived.by(() => {
    if (!selectedSkillId) return null;
    return idToNode.get(selectedSkillId)?.skill ?? null;
  });

  const neighborIds = $derived.by(() => {
    const ids = new Set<string>();
    if (!selectedSkillId) return ids;
    for (const edge of edges) {
      if (edge.from === selectedSkillId) ids.add(edge.to);
      if (edge.to === selectedSkillId) ids.add(edge.from);
    }
    return ids;
  });

  const selectedParent = $derived.by(() => {
    if (!selectedSkill?.parentId) return null;
    return idToNode.get(selectedSkill.parentId)?.skill ?? null;
  });

  const selectedChildren = $derived.by(() => {
    if (!selectedSkillId) return [];
    return nodes.filter((node) => node.skill.parentId === selectedSkillId).map((node) => node.skill);
  });

  const graphTransform = $derived(`translate(${panX} ${panY}) scale(${zoom})`);
  const zoomPercent = $derived(Math.round(zoom * 100));

  $effect(() => {
    rebuildGraph(skills);
  });

  $effect(() => {
    if (selectedSkillId && !idToNode.has(selectedSkillId)) {
      selectedSkillId = null;
    }
  });

  onDestroy(() => {
    cancelLayout();
    cancelViewAnimation();
    if (fitRaf != null) cancelAnimationFrame(fitRaf);
  });

  function rebuildGraph(nextSkills: Skill[]) {
    cancelLayout();

    const skillIds = new Set(nextSkills.map((skill) => skill.id));
    const nextEdges: GraphEdge[] = [];
    const degree = new Map<string, number>();
    for (const skill of nextSkills) degree.set(skill.id, 0);

    for (const skill of nextSkills) {
      if (skill.parentId && skillIds.has(skill.parentId)) {
        nextEdges.push({ from: skill.parentId, to: skill.id });
        degree.set(skill.parentId, (degree.get(skill.parentId) ?? 0) + 1);
        degree.set(skill.id, (degree.get(skill.id) ?? 0) + 1);
      }
    }

    const sorted = [...nextSkills].sort((a, b) => {
      const da = degree.get(a.id) ?? 0;
      const db = degree.get(b.id) ?? 0;
      return db - da || a.name.localeCompare(b.name);
    });

    const golden = Math.PI * (3 - Math.sqrt(5));
    const nextNodes = sorted.map((skill, index) => {
      const cached = positionCache.get(skill.id);
      const radius = 8 + Math.min(9, (degree.get(skill.id) ?? 0) * 1.4);
      const seedRadius = 36 + 18 * Math.sqrt(index + 1);
      const theta = index * golden;
      const agentId = typeof skill.agentId === "string" ? skill.agentId : "universal";

      return {
        id: skill.id,
        skill,
        x: cached?.x ?? CENTER_X + Math.cos(theta) * seedRadius,
        y: cached?.y ?? CENTER_Y + Math.sin(theta) * seedRadius,
        vx: 0,
        vy: 0,
        radius,
        degree: degree.get(skill.id) ?? 0,
        color: store.getAgentColor(agentId) ?? DEFAULT_AGENT_COLOR,
      };
    });
    nodes = nextNodes;
    edges = nextEdges;

    runLayout(layoutTickCount(nextNodes.length));
    if (fitRaf != null) cancelAnimationFrame(fitRaf);
    fitRaf = scheduleFrame(() => fitGraph(false));
  }

  function runLayout(maxTicks: number) {
    if (maxTicks <= 0) {
      nodes = [...nodes];
      return;
    }

    let tick = 0;

    const step = () => {
      tick += 1;
      const alpha = Math.max(0.05, 1 - tick / maxTicks);
      tickLayout(alpha);
      nodes = [...nodes];

      for (const node of nodes) {
        positionCache.set(node.id, { x: node.x, y: node.y });
      }

      if (!reducedMotion && tick < maxTicks) {
        layoutRaf = scheduleFrame(step);
      } else {
        layoutRaf = null;
      }
    };

    if (reducedMotion) {
      for (let i = 0; i < maxTicks; i += 1) tickLayout(Math.max(0.05, 1 - i / maxTicks));
      nodes = [...nodes];
      return;
    }

    layoutRaf = scheduleFrame(step);
  }

  function tickLayout(alpha: number) {
    const byId = new Map(nodes.map((node) => [node.id, node]));

    for (const edge of edges) {
      const from = byId.get(edge.from);
      const to = byId.get(edge.to);
      if (!from || !to) continue;

      const dx = to.x - from.x;
      const dy = to.y - from.y;
      const distance = Math.max(1, Math.hypot(dx, dy));
      const desired = 92 + Math.min(42, (from.radius + to.radius) * 1.4);
      const force = (distance - desired) * 0.012 * alpha;
      const fx = (dx / distance) * force;
      const fy = (dy / distance) * force;
      from.vx += fx;
      from.vy += fy;
      to.vx -= fx;
      to.vy -= fy;
    }

    if (nodes.length <= MAX_PAIRWISE_NODES) {
      for (let i = 0; i < nodes.length; i += 1) {
        for (let j = i + 1; j < nodes.length; j += 1) {
          const a = nodes[i];
          const b = nodes[j];
          let dx = b.x - a.x;
          let dy = b.y - a.y;
          let distance = Math.hypot(dx, dy);
          if (distance < 0.1) {
            dx = 0.1;
            dy = 0.1;
            distance = 0.14;
          }

          const minDistance = a.radius + b.radius + 28;
          const repulse = Math.min(1.8, 850 / (distance * distance)) * alpha;
          const collide = distance < minDistance ? (minDistance - distance) * 0.018 * alpha : 0;
          const force = repulse + collide;
          const fx = (dx / distance) * force;
          const fy = (dy / distance) * force;
          a.vx -= fx;
          a.vy -= fy;
          b.vx += fx;
          b.vy += fy;
        }
      }
    }

    for (const node of nodes) {
      node.vx += (CENTER_X - node.x) * 0.003 * alpha;
      node.vy += (CENTER_Y - node.y) * 0.003 * alpha;
      node.vx *= 0.82;
      node.vy *= 0.82;
      node.x = clamp(node.x + node.vx, 40, VIEWPORT_W - 40);
      node.y = clamp(node.y + node.vy, 40, VIEWPORT_H - 40);
    }
  }

  function cancelLayout() {
    if (layoutRaf != null) {
      cancelAnimationFrame(layoutRaf);
      layoutRaf = null;
    }
  }

  function cancelViewAnimation() {
    if (viewRaf != null) {
      cancelAnimationFrame(viewRaf);
      viewRaf = null;
    }
  }

  function animateView() {
    if (reducedMotion) {
      zoom = targetZoom;
      panX = targetPanX;
      panY = targetPanY;
      return;
    }

    cancelViewAnimation();
    const step = () => {
      zoom += (targetZoom - zoom) * 0.18;
      panX += (targetPanX - panX) * 0.18;
      panY += (targetPanY - panY) * 0.18;

      const done = Math.abs(targetZoom - zoom) < 0.002
        && Math.abs(targetPanX - panX) < 0.35
        && Math.abs(targetPanY - panY) < 0.35;

      if (done) {
        zoom = targetZoom;
        panX = targetPanX;
        panY = targetPanY;
        viewRaf = null;
      } else {
        viewRaf = scheduleFrame(step);
      }
    };
    viewRaf = scheduleFrame(step);
  }

  function setTargetView(nextZoom: number, nextPanX: number, nextPanY: number) {
    targetZoom = clamp(nextZoom, MIN_ZOOM, MAX_ZOOM);
    targetPanX = nextPanX;
    targetPanY = nextPanY;
    animateView();
  }

  function fitGraph(animated = true) {
    if (nodes.length === 0) {
      setTargetView(1, 0, 0);
      return;
    }

    const xs = nodes.map((node) => node.x);
    const ys = nodes.map((node) => node.y);
    const minX = Math.min(...xs);
    const maxX = Math.max(...xs);
    const minY = Math.min(...ys);
    const maxY = Math.max(...ys);
    const width = Math.max(120, maxX - minX + 120);
    const height = Math.max(120, maxY - minY + 120);
    const nextZoom = clamp(Math.min(VIEWPORT_W / width, VIEWPORT_H / height), MIN_ZOOM, 1.5);
    const centerX = (minX + maxX) / 2;
    const centerY = (minY + maxY) / 2;
    targetZoom = nextZoom;
    targetPanX = CENTER_X - centerX * nextZoom;
    targetPanY = CENTER_Y - centerY * nextZoom;

    if (animated) {
      animateView();
    } else {
      zoom = targetZoom;
      panX = targetPanX;
      panY = targetPanY;
    }
  }

  function focusNode(nodeId: string, nextZoom = Math.max(1.25, zoom)) {
    const node = idToNode.get(nodeId);
    if (!node) return;
    const clampedZoom = clamp(nextZoom, MIN_ZOOM, MAX_ZOOM);
    setTargetView(clampedZoom, CENTER_X - node.x * clampedZoom, CENTER_Y - node.y * clampedZoom);
  }

  function clientToSvgPoint(clientX: number, clientY: number): { x: number; y: number } | null {
    if (!svgEl) return null;
    const ctm = svgEl.getScreenCTM();
    if (!ctm) return null;
    const point = typeof DOMPoint !== "undefined"
      ? new DOMPoint(clientX, clientY).matrixTransform(ctm.inverse())
      : svgPointFallback(clientX, clientY, ctm);
    return { x: point.x, y: point.y };
  }

  function clientToWorldPoint(clientX: number, clientY: number): { x: number; y: number } | null {
    const point = clientToSvgPoint(clientX, clientY);
    if (!point) return null;
    return {
      x: (point.x - panX) / zoom,
      y: (point.y - panY) / zoom,
    };
  }

  function handleWheel(e: WheelEvent) {
    e.preventDefault();
    const svgPoint = clientToSvgPoint(e.clientX, e.clientY);
    const worldPoint = clientToWorldPoint(e.clientX, e.clientY);
    if (!svgPoint || !worldPoint) return;

    const nextZoom = clamp(zoom * Math.exp(-e.deltaY * 0.0014), MIN_ZOOM, MAX_ZOOM);
    setTargetView(
      nextZoom,
      svgPoint.x - worldPoint.x * nextZoom,
      svgPoint.y - worldPoint.y * nextZoom,
    );
  }

  function handlePointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    if (isNodeTarget(e.target)) return;

    isPanning = true;
    pointerId = e.pointerId;
    panMoved = false;
    panStartClientX = e.clientX;
    panStartClientY = e.clientY;
    panStartX = targetPanX;
    panStartY = targetPanY;
    svgEl?.setPointerCapture(e.pointerId);
    cancelViewAnimation();
  }

  function handlePointerMove(e: PointerEvent) {
    if (!isPanning || pointerId !== e.pointerId) return;
    const dx = e.clientX - panStartClientX;
    const dy = e.clientY - panStartClientY;
    if (Math.hypot(dx, dy) > 3) panMoved = true;
    targetPanX = panStartX + dx;
    targetPanY = panStartY + dy;
    panX = targetPanX;
    panY = targetPanY;
  }

  function handlePointerUp(e: PointerEvent) {
    if (!isPanning || pointerId !== e.pointerId) return;
    isPanning = false;
    pointerId = null;
    if (!panMoved && !isNodeTarget(e.target)) {
      selectedSkillId = null;
    }
  }

  function selectNode(node: GraphNode) {
    selectedSkillId = selectedSkillId === node.id ? null : node.id;
    if (selectedSkillId) focusNode(node.id);
  }

  function handleNodeKeydown(e: KeyboardEvent, node: GraphNode) {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      selectNode(node);
    } else if (e.key === "Escape") {
      selectedSkillId = null;
    }
  }

  function isNodeTarget(target: EventTarget | null): boolean {
    return target instanceof Element && !!target.closest("[data-graph-node]");
  }

  function isEdgeActive(edge: GraphEdge): boolean {
    if (!selectedSkillId && !hoveredSkillId) return false;
    const active = hoveredSkillId ?? selectedSkillId;
    return edge.from === active || edge.to === active;
  }

  function isNodeDimmed(node: GraphNode): boolean {
    if (!selectedSkillId && !hoveredSkillId) return false;
    const active = hoveredSkillId ?? selectedSkillId;
    if (node.id === active) return false;
    if (selectedSkillId && neighborIds.has(node.id)) return false;
    return true;
  }

  function showNodeLabel(node: GraphNode): boolean {
    return labelsEnabled
      || node.id === selectedSkillId
      || node.id === hoveredSkillId
      || (!!selectedSkillId && neighborIds.has(node.id))
      || zoom >= 1.75;
  }

  function zoomIn() {
    focusViewportZoom(1.16);
  }

  function zoomOut() {
    focusViewportZoom(1 / 1.16);
  }

  function focusViewportZoom(factor: number) {
    const worldX = (CENTER_X - panX) / zoom;
    const worldY = (CENTER_Y - panY) / zoom;
    const nextZoom = clamp(zoom * factor, MIN_ZOOM, MAX_ZOOM);
    setTargetView(nextZoom, CENTER_X - worldX * nextZoom, CENTER_Y - worldY * nextZoom);
  }

  function clamp(value: number, min: number, max: number): number {
    return Math.max(min, Math.min(max, value));
  }

  function layoutTickCount(count: number): number {
    if (reducedMotion) return Math.min(24, MAX_LAYOUT_TICKS);
    if (count <= 80) return MAX_LAYOUT_TICKS;
    if (count <= MAX_PAIRWISE_NODES) return 36;
    if (count <= 360) return 18;
    return 0;
  }

  function scheduleFrame(callback: FrameRequestCallback): number {
    if (typeof requestAnimationFrame === "function") {
      return requestAnimationFrame(callback);
    }
    return window.setTimeout(() => callback(performance.now()), 16);
  }

  function svgPointFallback(clientX: number, clientY: number, ctm: DOMMatrix): { x: number; y: number } {
    const inverse = ctm.inverse();
    return {
      x: inverse.a * clientX + inverse.c * clientY + inverse.e,
      y: inverse.b * clientX + inverse.d * clientY + inverse.f,
    };
  }
</script>

<div class="relative h-full overflow-hidden rounded-xl border"
  style="border-color: var(--color-border); background:
    radial-gradient(circle at 22% 16%, rgba(94, 106, 210, 0.13), transparent 30%),
    radial-gradient(circle at 78% 84%, rgba(94, 106, 210, 0.10), transparent 32%),
    linear-gradient(180deg, var(--color-surface-1), var(--color-surface-0));">
  {#if skills.length === 0}
    <div class="flex h-full items-center justify-center text-[12px] text-[var(--color-text-secondary)]">
      No skills to graph
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <svg
      class="h-full w-full"
      style="cursor: {isPanning ? 'grabbing' : 'grab'}; touch-action: none;"
      viewBox={`0 0 ${VIEWPORT_W} ${VIEWPORT_H}`}
      preserveAspectRatio="xMidYMid meet"
      bind:this={svgEl}
      onwheel={handleWheel}
      onpointerdown={handlePointerDown}
      onpointermove={handlePointerMove}
      onpointerup={handlePointerUp}
      onpointercancel={handlePointerUp}
    >
      <defs>
        <pattern id="graph-grid" width="38" height="38" patternUnits="userSpaceOnUse">
          <path d="M 38 0 L 0 0 0 38" fill="none" stroke="var(--color-border)" stroke-opacity="0.22" stroke-width="1" />
        </pattern>
        <filter id="node-glow" x="-80%" y="-80%" width="260%" height="260%">
          <feGaussianBlur stdDeviation="5" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>

      <rect x="0" y="0" width={VIEWPORT_W} height={VIEWPORT_H} fill="url(#graph-grid)" opacity="0.34" />

      <g transform={graphTransform}>
        {#each edges as edge (edge.from + '->' + edge.to)}
          {@const from = idToNode.get(edge.from)}
          {@const to = idToNode.get(edge.to)}
          {#if from && to}
            <line
              x1={from.x}
              y1={from.y}
              x2={to.x}
              y2={to.y}
              stroke={isEdgeActive(edge) ? "var(--color-accent)" : "var(--color-border-hover)"}
              stroke-width={isEdgeActive(edge) ? 2.4 / zoom : 1.1 / zoom}
              stroke-opacity={isEdgeActive(edge) ? "0.92" : "0.34"}
              style="transition: stroke 160ms ease, stroke-opacity 160ms ease;"
            />
          {/if}
        {/each}

        {#each nodes as node (node.id)}
          {@const isSelected = selectedSkillId === node.id}
          {@const isHovered = hoveredSkillId === node.id}
          {@const dimmed = isNodeDimmed(node)}
          <g
            role="button"
            tabindex="0"
            aria-label={`Select ${node.skill.name}`}
            data-graph-node="true"
            transform={`translate(${node.x} ${node.y})`}
            onclick={(e) => { e.stopPropagation(); selectNode(node); }}
            ondblclick={(e) => { e.stopPropagation(); selectedSkillId = node.id; focusNode(node.id, 1.85); }}
            onkeydown={(e) => handleNodeKeydown(e, node)}
            onpointerenter={() => (hoveredSkillId = node.id)}
            onpointerleave={() => (hoveredSkillId = null)}
            style="cursor:pointer; outline:none;"
          >
            <circle r={node.radius + 16} fill="transparent" />
            <circle
              r={node.radius + (isSelected ? 13 : isHovered ? 8 : 4)}
              fill={node.color}
              opacity={isSelected ? "0.30" : isHovered ? "0.20" : "0.09"}
              filter={isSelected || isHovered ? "url(#node-glow)" : undefined}
              pointer-events="none"
            />
            <circle
              r={node.radius}
              fill={isSelected ? "var(--color-accent)" : "var(--color-surface-3)"}
              stroke={node.color}
              stroke-width={isSelected ? 2.6 / zoom : 1.35 / zoom}
              opacity={dimmed ? "0.28" : "1"}
              pointer-events="none"
              style="transition: opacity 160ms ease, fill 160ms ease, stroke-width 160ms ease;"
            />
            {#if node.degree > 0}
              <circle
                r={Math.max(2.2, node.radius * 0.22)}
                fill={node.color}
                opacity={dimmed ? "0.3" : "0.95"}
                pointer-events="none"
              />
            {/if}
            {#if showNodeLabel(node)}
              <text
                x={node.radius + 9}
                y="4"
                fill="var(--color-text-primary)"
                font-size={11 / Math.max(0.9, zoom)}
                font-weight="600"
                paint-order="stroke"
                stroke="var(--color-surface-0)"
                stroke-width={4 / zoom}
                pointer-events="none"
              >{node.skill.name}</text>
            {/if}
          </g>
        {/each}
      </g>
    </svg>

    <div class="absolute left-3 top-3 flex items-center gap-1 rounded-lg border p-1 backdrop-blur"
      style="border-color: var(--color-border); background: color-mix(in srgb, var(--color-surface-2) 88%, transparent);">
      <button class="graph-control" onclick={zoomOut} aria-label="Zoom out">−</button>
      <button class="graph-control min-w-12" onclick={() => fitGraph(true)} aria-label="Fit graph">{zoomPercent}%</button>
      <button class="graph-control" onclick={zoomIn} aria-label="Zoom in">+</button>
      <span class="mx-1 h-4 w-px bg-[var(--color-border)]"></span>
      <button class="graph-control" onclick={() => fitGraph(true)} aria-label="Fit all nodes">fit</button>
      <button
        class="graph-control {labelsEnabled ? 'text-[var(--color-accent)]' : ''}"
        onclick={() => (labelsEnabled = !labelsEnabled)}
        aria-label="Toggle labels"
      >Aa</button>
    </div>

    <div class="absolute right-3 top-3 rounded-lg border px-2.5 py-1.5 text-[10px] text-[var(--color-text-muted)] backdrop-blur"
      style="border-color: var(--color-border); background: color-mix(in srgb, var(--color-surface-2) 88%, transparent);">
      {nodes.length} skills · {edges.length} links
    </div>

    {#if selectedSkill}
      <div class="absolute bottom-3 right-3 w-[270px] rounded-xl border p-3 shadow-xl backdrop-blur"
        style="border-color: var(--color-border-active); background: color-mix(in srgb, var(--color-surface-2) 94%, transparent); box-shadow: 0 18px 40px -20px var(--color-overlay-shadow);">
        <div class="flex items-start justify-between gap-2">
          <div class="min-w-0">
            <div class="flex items-center gap-1.5">
              <span class="h-2 w-2 rounded-full" style="background: {idToNode.get(selectedSkill.id)?.color ?? DEFAULT_AGENT_COLOR};"></span>
              <h3 class="truncate text-[12px] font-semibold text-[var(--color-text-primary)]">{selectedSkill.name}</h3>
            </div>
            <p class="mt-0.5 text-[10px] text-[var(--color-text-muted)]">
              {store.getAgentDisplayName(typeof selectedSkill.agentId === "string" ? selectedSkill.agentId : "universal")}
            </p>
          </div>
          <button class="graph-control" onclick={() => (selectedSkillId = null)} aria-label="Clear selection">×</button>
        </div>

        <p class="mt-2 line-clamp-3 text-[11px] leading-relaxed text-[var(--color-text-secondary)]">
          {selectedSkill.description || "No description"}
        </p>

        <div class="mt-2 flex flex-wrap gap-1.5 text-[9px] text-[var(--color-text-muted)]">
          {#if selectedParent}
            <span class="graph-chip">parent: {selectedParent.name}</span>
          {/if}
          <span class="graph-chip">children: {selectedChildren.length}</span>
          {#if selectedSkill.metadata.trigger}
            <span class="graph-chip">{selectedSkill.metadata.trigger}</span>
          {/if}
          <span class="graph-chip">{selectedSkill.scope}</span>
        </div>

        {#if selectedChildren.length > 0}
          <div class="mt-2 flex flex-wrap gap-1">
            {#each selectedChildren.slice(0, 4) as child (child.id)}
              <button
                class="max-w-[116px] truncate rounded px-1.5 py-0.5 text-[9px] text-[var(--color-text-secondary)] hover:text-[var(--color-accent)]"
                style="background: var(--color-surface-3); border: 1px solid var(--color-border);"
                onclick={() => { selectedSkillId = child.id; focusNode(child.id); }}
                title={child.name}
              >{child.name}</button>
            {/each}
          </div>
        {/if}

        <div class="mt-3 flex items-center justify-between gap-2">
          <button class="graph-action" onclick={() => focusNode(selectedSkill.id, 1.65)}>center</button>
          <button class="graph-action" onclick={() => copySkillReference(selectedSkill)}>copy reference</button>
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  .graph-control {
    height: 24px;
    border-radius: var(--radius-sm);
    padding: 0 8px;
    color: var(--color-text-secondary);
    font-size: 10px;
    font-weight: 600;
    transition: background 140ms ease, color 140ms ease, transform 140ms ease;
  }

  .graph-control:hover {
    background: var(--color-surface-3);
    color: var(--color-text-primary);
  }

  .graph-control:active {
    transform: scale(0.95);
  }

  .graph-chip {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-sm);
    background: var(--color-surface-3);
    padding: 2px 6px;
  }

  .graph-action {
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-border);
    background: var(--color-surface-3);
    padding: 4px 8px;
    color: var(--color-text-secondary);
    font-size: 10px;
    font-weight: 600;
    transition: border-color 140ms ease, color 140ms ease, background 140ms ease;
  }

  .graph-action:hover {
    border-color: var(--color-accent-muted);
    color: var(--color-accent);
  }
</style>
