/**
 * Universal interactive GraphViewer client module.
 *
 * Provides canvas-based graph visualization, force-directed simulation,
 * tabular data inspection, multi-dialect query viewing, and bidirectional
 * AnyWidget traitlet synchronization.
 */

/**
 * Renders the Voyager GraphViewer widget into the specified container element.
 *
 * @param {Object} options - AnyWidget render context.
 * @param {Object} options.model - Traitlet model providing reactive graph state and event handlers.
 * @param {HTMLElement} options.el - Target DOM element for mounting the viewer root.
 * @returns {Function} Teardown callback invoked when the widget view is destroyed.
 */
export function render({ model, el }) {
  /**
   * Resolves the active color theme preference from model settings or notebook environment.
   * @returns {"light" | "dark"}
   */
  function getActiveTheme() {
    const pref = model.get("theme") || "light";
    if (pref === "dark") return "dark";
    if (pref === "light") return "light";

    const isDarkEnv =
      document.body.classList.contains("dark") ||
      document.body.classList.contains("vscode-dark") ||
      document.documentElement.getAttribute("data-theme") === "dark" ||
      document.documentElement.classList.contains("theme-dark") ||
      document.documentElement.classList.contains("jp-theme-dark") ||
      (window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches);

    return isDarkEnv ? "dark" : "light";
  }

  const isLight = getActiveTheme() === "light";

  /**
   * Color tokens utilized for Canvas 2D rasterization.
   * DOM elements inherit their styling from CSS custom properties in viewer.css.
   */
  const theme = {
    bgRoot: isLight ? "#f8fafc" : "#080c14",
    bgNav: isLight ? "#ffffff" : "#0d131f",
    bgSurface: isLight ? "rgba(255, 255, 255, 0.94)" : "rgba(13, 19, 31, 0.94)",
    bgSubtle: isLight ? "#f1f5f9" : "#131b2c",
    bgHover: isLight ? "#e2e8f0" : "#1e293b",
    textPrimary: isLight ? "#0f172a" : "#f8fafc",
    textMuted: isLight ? "#64748b" : "#94a3b8",
    borderSubtle: isLight ? "rgba(0, 0, 0, 0.08)" : "rgba(255, 255, 255, 0.08)",
    borderStrong: isLight ? "rgba(0, 0, 0, 0.16)" : "rgba(255, 255, 255, 0.16)",
    accent: isLight ? "#0284c7" : "#38bdf8",
    accentHover: isLight ? "#0369a1" : "#7dd3fc",
    accentGlow: isLight ? "rgba(2, 132, 199, 0.2)" : "rgba(56, 189, 248, 0.25)",
    accentText: isLight ? "#ffffff" : "#080c14",
    badgeBg: isLight ? "#e2e8f0" : "#131b2c",
    dotGrid: isLight ? "rgba(100, 116, 139, 0.16)" : "rgba(148, 163, 184, 0.12)",
    success: "#10b981",
    selection: "#f43f5e",
  };

  /**
   * Escapes HTML entities in strings to prevent script injection.
   * @param {*} str
   * @returns {string}
   */
  function escapeHtml(str) {
    if (str === null || str === undefined) return "";
    return String(str)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;")
      .replace(/'/g, "&#039;");
  }

  /** Micro-icon SVG definitions for toolbar and UI controls. */
  const icons = {
    search: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>`,
    copy: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/></svg>`,
    check: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#10b981" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6 9 17l-5-5"/></svg>`,
    chevronLeft: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m15 18-6-6 6-6"/></svg>`,
    chevronRight: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="m9 18 6-6-6-6"/></svg>`,
    close: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>`,
    download: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/></svg>`,
    maximize: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3H5a2 2 0 0 0-2 2v3"/><path d="M21 8V5a2 2 0 0 0-2-2h-3"/><path d="M3 16v3a2 2 0 0 0 2 2h3"/><path d="M16 21h3a2 2 0 0 0 2-2v-3"/></svg>`,
    play: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="6 3 20 12 6 21 6 3"/></svg>`,
    pause: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg>`,
    layers: `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="12 2 2 7 12 12 22 7 12 2"/><polyline points="2 17 12 22 22 17"/><polyline points="2 12 12 17 22 12"/></svg>`,
  };

  // Root container initialization
  const root = document.createElement("div");
  root.className = "voyager-root";
  root.dataset.theme = isLight ? "light" : "dark";
  root.tabIndex = 0;
  root.style.height = model.get("height") || "620px";

  // Navigation header
  const nav = document.createElement("div");
  nav.className = "voyager-nav";
  root.appendChild(nav);

  const leftSection = document.createElement("div");
  leftSection.style.display = "flex";
  leftSection.style.alignItems = "center";
  leftSection.style.gap = "8px";
  leftSection.style.flex = "1";
  nav.appendChild(leftSection);

  const rawNodes = model.get("nodes") || [];
  const rawEdges = model.get("edges") || [];
  const rawRecords = model.get("records") || [];
  const rawTypes = model.get("column_types") || {};
  let defaultView = model.get("default_view") || "auto";

  let activeTab = "graph";
  if (defaultView === "table" || (defaultView === "auto" && rawEdges.length === 0 && rawRecords.length > 0 && rawNodes.length === 0)) {
    activeTab = "table";
  } else if (defaultView === "query") {
    activeTab = "query";
  }

  // Summary badge in header
  const statsBadge = document.createElement("div");
  statsBadge.className = "voyager-badge";
  statsBadge.textContent = `${rawNodes.length} Nodes  ·  ${rawEdges.length} Edges  ·  ${rawRecords.length} Rows`;
  leftSection.appendChild(statsBadge);

  // Tab switcher
  const centerSection = document.createElement("div");
  centerSection.style.display = "flex";
  centerSection.style.justifyContent = "center";
  nav.appendChild(centerSection);

  const switcher = document.createElement("div");
  switcher.className = "voyager-tab-switcher";
  centerSection.appendChild(switcher);

  function createTabButton(id, label) {
    const btn = document.createElement("button");
    btn.className = `voyager-tab-btn ${activeTab === id ? "active" : ""}`;
    btn.textContent = label;
    btn.addEventListener("click", () => switchTab(id));
    return btn;
  }

  const tabGraph = createTabButton("graph", "Graph");
  const tabTable = createTabButton("table", "Table");
  const tabQuery = createTabButton("query", "Query");
  switcher.appendChild(tabGraph);
  switcher.appendChild(tabTable);
  switcher.appendChild(tabQuery);

  // Right-aligned toolbar
  const rightSection = document.createElement("div");
  rightSection.style.display = "flex";
  rightSection.style.alignItems = "center";
  rightSection.style.justifyContent = "flex-end";
  rightSection.style.gap = "6px";
  rightSection.style.flex = "1";
  nav.appendChild(rightSection);

  const toolbar = document.createElement("div");
  toolbar.style.display = activeTab === "graph" ? "flex" : "none";
  toolbar.style.alignItems = "center";
  toolbar.style.gap = "6px";
  rightSection.appendChild(toolbar);

  // Node search input
  const searchWrap = document.createElement("div");
  searchWrap.className = "voyager-search-wrap";

  const searchIconEl = document.createElement("span");
  searchIconEl.className = "voyager-search-icon";
  searchIconEl.innerHTML = icons.search;
  searchWrap.appendChild(searchIconEl);

  const searchInput = document.createElement("input");
  searchInput.type = "text";
  searchInput.className = "voyager-input";
  searchInput.placeholder = "Search nodes...";
  searchInput.style.paddingLeft = "26px";
  searchInput.style.width = "130px";

  searchInput.addEventListener("focus", () => {
    searchInput.style.width = "180px";
  });
  searchInput.addEventListener("blur", () => {
    if (!searchInput.value) searchInput.style.width = "130px";
  });

  searchWrap.appendChild(searchInput);
  toolbar.appendChild(searchWrap);

  // Layout selector dropdown
  const layoutSelect = document.createElement("select");
  layoutSelect.className = "voyager-input voyager-select";
  layoutSelect.style.cursor = "pointer";
  [
    { val: "force", label: "Force-Directed" },
    { val: "circular", label: "Circular" },
    { val: "hierarchical", label: "Hierarchical" },
  ].forEach(opt => {
    const o = document.createElement("option");
    o.value = opt.val;
    o.textContent = opt.label;
    layoutSelect.appendChild(o);
  });
  toolbar.appendChild(layoutSelect);

  // Physics simulation pause button
  const pauseBtn = document.createElement("button");
  pauseBtn.className = "voyager-btn";
  pauseBtn.innerHTML = `<span>Pause</span>`;
  pauseBtn.title = "Pause / Resume (Shortcut: P)";
  toolbar.appendChild(pauseBtn);

  // Canvas fit view button
  const fitBtn = document.createElement("button");
  fitBtn.className = "voyager-btn";
  fitBtn.innerHTML = `${icons.maximize} <span>Fit</span>`;
  fitBtn.title = "Fit View (Shortcut: F)";
  toolbar.appendChild(fitBtn);

  // PNG snapshot export button
  const exportPngBtn = document.createElement("button");
  exportPngBtn.className = "voyager-btn";
  exportPngBtn.innerHTML = `${icons.download} <span>PNG</span>`;
  exportPngBtn.title = "Save high-resolution snapshot of graph canvas";
  toolbar.appendChild(exportPngBtn);

  // Main viewport content area
  const contentArea = document.createElement("div");
  contentArea.style.flex = "1";
  contentArea.style.position = "relative";
  contentArea.style.overflow = "hidden";
  root.appendChild(contentArea);

  // Tab 1: Interactive Canvas Container
  const canvasContainer = document.createElement("div");
  canvasContainer.style.width = "100%";
  canvasContainer.style.height = "100%";
  canvasContainer.style.position = "relative";
  canvasContainer.style.display = activeTab === "graph" ? "block" : "none";
  contentArea.appendChild(canvasContainer);

  const canvas = document.createElement("canvas");
  canvas.style.width = "100%";
  canvas.style.height = "100%";
  canvas.style.display = "block";
  canvasContainer.appendChild(canvas);

  // Collapsible label filtering panel
  let isLeftPanelOpen = true;

  const leftPanel = document.createElement("div");
  leftPanel.className = "voyager-left-panel";
  canvasContainer.appendChild(leftPanel);

  // Floating trigger button to re-open collapsed label panel
  const toggleLeftPanelBtn = document.createElement("button");
  toggleLeftPanelBtn.className = "voyager-btn";
  toggleLeftPanelBtn.style.position = "absolute";
  toggleLeftPanelBtn.style.top = "12px";
  toggleLeftPanelBtn.style.left = "12px";
  toggleLeftPanelBtn.style.zIndex = "19";
  toggleLeftPanelBtn.style.background = theme.bgSurface;
  toggleLeftPanelBtn.style.backdropFilter = "blur(10px)";
  toggleLeftPanelBtn.style.fontWeight = "600";
  toggleLeftPanelBtn.style.fontSize = "11px";
  toggleLeftPanelBtn.style.display = "none";
  toggleLeftPanelBtn.style.boxShadow = "0 4px 12px rgba(0, 0, 0, 0.15)";
  toggleLeftPanelBtn.innerHTML = `<span>Labels</span> ${icons.chevronRight}`;
  canvasContainer.appendChild(toggleLeftPanelBtn);

  function setLeftPanelOpen(open) {
    isLeftPanelOpen = open;
    if (open) {
      leftPanel.style.transform = "translateX(0)";
      leftPanel.style.opacity = "1";
      leftPanel.style.pointerEvents = "auto";
      toggleLeftPanelBtn.style.display = "none";
      hudPill.style.left = "244px";
    } else {
      leftPanel.style.transform = "translateX(-240px)";
      leftPanel.style.opacity = "0";
      leftPanel.style.pointerEvents = "none";
      toggleLeftPanelBtn.style.display = "inline-flex";
      hudPill.style.left = "12px";
    }
  }

  toggleLeftPanelBtn.addEventListener("click", () => setLeftPanelOpen(true));

  // Left panel header
  const lpHeader = document.createElement("div");
  lpHeader.style.display = "flex";
  lpHeader.style.justifyContent = "space-between";
  lpHeader.style.alignItems = "center";
  lpHeader.style.marginBottom = "8px";

  const lpTitle = document.createElement("div");
  lpTitle.style.fontSize = "12px";
  lpTitle.style.fontWeight = "600";
  lpTitle.style.display = "flex";
  lpTitle.style.alignItems = "center";
  lpTitle.style.gap = "6px";
  lpTitle.innerHTML = `${icons.layers} <span>Node Labels</span>`;
  lpHeader.appendChild(lpTitle);

  const lpCloseBtn = document.createElement("button");
  lpCloseBtn.className = "voyager-btn";
  lpCloseBtn.innerHTML = icons.chevronLeft;
  lpCloseBtn.title = "Collapse Panel";
  lpCloseBtn.style.background = "transparent";
  lpCloseBtn.style.border = "none";
  lpCloseBtn.style.color = theme.textMuted;
  lpCloseBtn.style.padding = "4px";
  lpCloseBtn.addEventListener("click", () => setLeftPanelOpen(false));
  lpHeader.appendChild(lpCloseBtn);
  leftPanel.appendChild(lpHeader);

  // Left panel batch filter buttons
  const lpActions = document.createElement("div");
  lpActions.style.display = "flex";
  lpActions.style.gap = "6px";
  lpActions.style.marginBottom = "8px";

  const selectAllBtn = document.createElement("button");
  selectAllBtn.className = "voyager-btn";
  selectAllBtn.textContent = "All";
  selectAllBtn.style.flex = "1";
  selectAllBtn.style.fontSize = "11px";
  selectAllBtn.style.padding = "4px 0";
  selectAllBtn.style.minHeight = "24px";

  const deselectAllBtn = document.createElement("button");
  deselectAllBtn.className = "voyager-btn";
  deselectAllBtn.textContent = "None";
  deselectAllBtn.style.flex = "1";
  deselectAllBtn.style.fontSize = "11px";
  deselectAllBtn.style.padding = "4px 0";
  deselectAllBtn.style.minHeight = "24px";

  lpActions.appendChild(selectAllBtn);
  lpActions.appendChild(deselectAllBtn);
  leftPanel.appendChild(lpActions);

  // Scrollable labels list
  const labelsListContainer = document.createElement("div");
  labelsListContainer.style.flex = "1";
  labelsListContainer.style.overflowY = "auto";
  labelsListContainer.style.paddingRight = "2px";
  leftPanel.appendChild(labelsListContainer);

  // HUD zoom and simulation status pill
  const hudPill = document.createElement("div");
  hudPill.className = "voyager-hud";
  hudPill.innerHTML = `
    <span class="v-hud-status">Settled</span> &middot;
    <span class="v-hud-zoom">100%</span>
    <span style="display:inline-flex; gap:4px; margin-left:4px;">
      <button class="voyager-btn v-zoom-in" style="background:transparent; border:none; color:${theme.textPrimary}; font-size:14px; font-weight:600; width:24px; height:24px; padding:0;">+</button>
      <button class="voyager-btn v-zoom-out" style="background:transparent; border:none; color:${theme.textPrimary}; font-size:14px; font-weight:600; width:24px; height:24px; padding:0;">&minus;</button>
      <button class="voyager-btn v-zoom-reset" style="background:transparent; border:none; color:${theme.accent}; font-size:11px; font-weight:600; padding:0 4px; height:24px;">1:1</button>
    </span>
  `;
  canvasContainer.appendChild(hudPill);

  const statusEl = hudPill.querySelector(".v-hud-status");
  const zoomEl = hudPill.querySelector(".v-hud-zoom");

  // Tab 2: Tabular Records Container
  const tableContainer = document.createElement("div");
  tableContainer.className = "voyager-table-wrap";
  tableContainer.style.display = activeTab === "table" ? "flex" : "none";
  contentArea.appendChild(tableContainer);

  // Tab 3: Multi-Dialect Query Statements Container
  const queryContainer = document.createElement("div");
  queryContainer.className = "voyager-query-wrap";
  queryContainer.style.display = activeTab === "query" ? "block" : "none";
  contentArea.appendChild(queryContainer);

  // Floating Glass Inspector Drawer for selected entities
  const inspector = document.createElement("div");
  inspector.className = "voyager-inspector";
  canvasContainer.appendChild(inspector);

  // Canvas hover tooltip
  const tooltip = document.createElement("div");
  tooltip.className = "voyager-tooltip";
  canvasContainer.appendChild(tooltip);

  el.appendChild(root);

  // Simulation, transform, and selection state
  let width = 800;
  let height = 550;
  let transform = { x: 0, y: 0, k: 1 };
  let draggedNode = null;
  let hoveredEntity = null;
  let selectedEntity = null;
  let isPanning = false;
  let panStart = { x: 0, y: 0 };
  let dragDisplacement = 0;
  let searchTerm = "";
  let currentLayout = "force";
  let isPhysicsPaused = false;
  let simAlpha = 1.0;
  let isSimulating = false;
  let rafId = null;
  let tweenRafId = null;
  let isClosingInspector = false;
  const activeLabelFilters = new Set();
  const labelItemElements = new Map();

  const ctx = canvas.getContext("2d");

  // Distinct categorical color palettes for graph entity types
  const paletteDark = ["#00e5ff", "#10b981", "#f59e0b", "#ec4899", "#8b5cf6", "#f97316", "#06b6d4", "#a855f7"];
  const paletteLight = ["#0284c7", "#059669", "#d97706", "#db2777", "#7c3aed", "#ea580c", "#0891b2", "#9333ea"];
  const currentPalette = isLight ? paletteLight : paletteDark;

  const labelColorMap = new Map();
  const labelCounts = new Map();

  function getColorForLabel(lbl) {
    if (!lbl) return currentPalette[0];
    if (!labelColorMap.has(lbl)) {
      const idx = labelColorMap.size % currentPalette.length;
      labelColorMap.set(lbl, currentPalette[idx]);
    }
    return labelColorMap.get(lbl);
  }

  // Parse nodes and initialize spatial distribution
  const nodeMap = new Map();
  const nodes = rawNodes.map((n, i) => {
    const angle = (i / Math.max(rawNodes.length, 1)) * 2 * Math.PI;
    const radius = Math.min(width, height) * 0.32;
    const lbl = n.label || String(n.id);
    const grp = n.group || lbl;
    labelCounts.set(grp, (labelCounts.get(grp) || 0) + 1);

    const nodeObj = {
      id: String(n.id),
      label: lbl,
      group: grp,
      color: n.color || getColorForLabel(grp),
      baseSize: n.size || 11,
      size: n.size || 11,
      data: n.data || {},
      x: width / 2 + radius * Math.cos(angle) + (Math.random() - 0.5) * 30,
      y: height / 2 + radius * Math.sin(angle) + (Math.random() - 0.5) * 30,
      targetX: null,
      targetY: null,
      startX: null,
      startY: null,
      vx: 0,
      vy: 0,
      inDegree: 0,
      outDegree: 0,
      neighbors: new Set(),
    };
    nodeMap.set(nodeObj.id, nodeObj);
    return nodeObj;
  });

  // Parse edges and compute multi-edge pair indexing
  const edgePairMap = new Map();
  const edgeTypeCounts = new Map();

  const edges = rawEdges.map((e, idx) => {
    const src = nodeMap.get(String(e.source));
    const tgt = nodeMap.get(String(e.target));
    if (src && tgt) {
      src.outDegree += 1;
      tgt.inDegree += 1;
      src.neighbors.add(tgt.id);
      tgt.neighbors.add(src.id);

      const relType = e.label || "CONNECTED_TO";
      edgeTypeCounts.set(relType, (edgeTypeCounts.get(relType) || 0) + 1);

      const isReverse = src.id > tgt.id;
      const canonicalKey = isReverse ? `${tgt.id}::${src.id}` : `${src.id}::${tgt.id}`;
      const count = edgePairMap.get(canonicalKey) || 0;
      edgePairMap.set(canonicalKey, count + 1);

      return {
        id: e.id || `edge_${idx}_${src.id}_${tgt.id}`,
        source: src,
        target: tgt,
        label: e.label || "",
        color: e.color || (isLight ? "#94a3b8" : "#64748b"),
        data: e.data || {},
        pairIndex: count,
        isReverse: isReverse,
      };
    }
    return null;
  }).filter(Boolean);

  // Scale node visual radius proportionally to degree centrality
  nodes.forEach(n => {
    const degree = n.inDegree + n.outDegree;
    n.size = Math.min(26, Math.max(9, n.baseSize + Math.sqrt(degree) * 2.2));
  });

  // Populate node label list items in left panel
  labelCounts.forEach((count, grp) => {
    const col = getColorForLabel(grp);
    const item = document.createElement("div");
    item.className = "voyager-label-item";

    const leftSpan = document.createElement("div");
    leftSpan.style.display = "flex";
    leftSpan.style.alignItems = "center";
    leftSpan.style.gap = "6px";

    const dot = document.createElement("span");
    dot.style.display = "inline-block";
    dot.style.width = "8px";
    dot.style.height = "8px";
    dot.style.borderRadius = "50%";
    dot.style.background = col;
    dot.style.boxShadow = `0 0 6px ${col}66`;

    const lblSpan = document.createElement("span");
    lblSpan.style.color = theme.textPrimary;
    lblSpan.textContent = grp;

    leftSpan.appendChild(dot);
    leftSpan.appendChild(lblSpan);

    const cntBadge = document.createElement("span");
    cntBadge.style.fontSize = "10px";
    cntBadge.style.color = theme.textMuted;
    cntBadge.style.background = theme.badgeBg;
    cntBadge.style.padding = "1px 5px";
    cntBadge.style.borderRadius = "4px";
    cntBadge.textContent = String(count);

    item.appendChild(leftSpan);
    item.appendChild(cntBadge);

    item.addEventListener("click", () => {
      if (activeLabelFilters.has(grp)) {
        activeLabelFilters.delete(grp);
        item.classList.remove("dimmed");
      } else {
        activeLabelFilters.add(grp);
        item.classList.add("dimmed");
      }
      requestDraw();
    });

    labelItemElements.set(grp, item);
    labelsListContainer.appendChild(item);
  });

  // Populate relationship types list in left panel
  if (edgeTypeCounts.size > 0) {
    const edgeDivider = document.createElement("div");
    edgeDivider.style.fontSize = "11px";
    edgeDivider.style.fontWeight = "600";
    edgeDivider.style.color = theme.textMuted;
    edgeDivider.style.marginTop = "12px";
    edgeDivider.style.marginBottom = "6px";
    edgeDivider.textContent = "Edge Types";
    labelsListContainer.appendChild(edgeDivider);

    edgeTypeCounts.forEach((count, relType) => {
      const eItem = document.createElement("div");
      eItem.className = "voyager-label-item";
      eItem.style.cursor = "default";

      const leftSpan = document.createElement("div");
      leftSpan.style.display = "flex";
      leftSpan.style.alignItems = "center";
      leftSpan.style.gap = "6px";

      const line = document.createElement("span");
      line.style.display = "inline-block";
      line.style.width = "10px";
      line.style.height = "2px";
      line.style.background = isLight ? "#94a3b8" : "#64748b";

      const lblSpan = document.createElement("span");
      lblSpan.style.color = theme.textPrimary;
      lblSpan.className = "voyager-mono";
      lblSpan.style.fontSize = "10px";
      lblSpan.textContent = `[:${relType}]`;

      leftSpan.appendChild(line);
      leftSpan.appendChild(lblSpan);

      const cntBadge = document.createElement("span");
      cntBadge.style.fontSize = "10px";
      cntBadge.style.color = theme.textMuted;
      cntBadge.style.background = theme.badgeBg;
      cntBadge.style.padding = "1px 5px";
      cntBadge.style.borderRadius = "4px";
      cntBadge.textContent = String(count);

      eItem.appendChild(leftSpan);
      eItem.appendChild(cntBadge);
      labelsListContainer.appendChild(eItem);
    });
  }

  selectAllBtn.addEventListener("click", () => {
    activeLabelFilters.clear();
    labelItemElements.forEach((el) => el.classList.remove("dimmed"));
    requestDraw();
  });

  deselectAllBtn.addEventListener("click", () => {
    labelCounts.forEach((_, grp) => activeLabelFilters.add(grp));
    labelItemElements.forEach((el) => el.classList.add("dimmed"));
    requestDraw();
  });

  /**
   * Resets simulation energy and restarts the animation loop if needed.
   */
  function wakeSimulation() {
    if (isPhysicsPaused || currentLayout !== "force") return;
    simAlpha = 1.0;
    if (!isSimulating) {
      isSimulating = true;
      if (statusEl) statusEl.textContent = "Simulating...";
      runLoop();
    }
  }

  /**
   * Triggers a single render frame when the physics simulation is at rest.
   */
  function requestDraw() {
    if (!isSimulating) {
      draw();
    }
  }

  /**
   * Toggles physics calculation on/off.
   */
  function togglePhysics() {
    isPhysicsPaused = !isPhysicsPaused;
    pauseBtn.innerHTML = isPhysicsPaused ? `<span>Resume</span>` : `<span>Pause</span>`;
    if (!isPhysicsPaused) wakeSimulation();
  }
  pauseBtn.addEventListener("click", togglePhysics);

  /**
   * Closes the inspector drawer with animated slide-out and synchronizes model traitlets.
   * Uses a re-entry guard to prevent recursive loops during traitlet synchronization.
   *
   * @param {boolean} [syncModel=true] - Whether to clear model traitlets on close.
   */
  function closeInspector(syncModel = true) {
    if (isClosingInspector) return;
    if (!selectedEntity && inspector.style.display === "none") return;
    isClosingInspector = true;
    try {
      inspector.style.opacity = "0";
      inspector.style.transform = "translateX(20px)";
      setTimeout(() => {
        if (isClosingInspector || !selectedEntity) {
          inspector.style.display = "none";
        }
      }, 200);
      selectedEntity = null;
      if (syncModel && model) {
        let changed = false;
        if (model.get("selected_node")) {
          model.set("selected_node", "");
          changed = true;
        }
        if (model.get("selected_edge")) {
          model.set("selected_edge", "");
          changed = true;
        }
        if (changed) {
          model.save_changes();
        }
      }
      requestDraw();
    } finally {
      isClosingInspector = false;
    }
  }

  /**
   * Adjusts zoom and pan transform to center all graph nodes within the viewport.
   */
  function fitView() {
    if (nodes.length === 0) return;
    let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
    nodes.forEach(n => {
      if (Number.isFinite(n.x) && Number.isFinite(n.y)) {
        minX = Math.min(minX, n.x - 30);
        maxX = Math.max(maxX, n.x + 30);
        minY = Math.min(minY, n.y - 30);
        maxY = Math.max(maxY, n.y + 30);
      }
    });
    const gw = maxX - minX || 1;
    const gh = maxY - minY || 1;
    const scale = Math.min(Math.max((width - 80) / gw, 0.15), Math.max((height - 80) / gh, 0.15), 1.6);
    transform.k = scale;
    transform.x = width / 2 - ((minX + maxX) / 2) * scale;
    transform.y = height / 2 - ((minY + maxY) / 2) * scale;
    if (zoomEl) zoomEl.textContent = `${Math.round(transform.k * 100)}%`;
    requestDraw();
  }

  /**
   * Handles canvas buffer resizing accounting for device pixel ratio.
   */
  function resize() {
    width = canvasContainer.clientWidth || 800;
    height = canvasContainer.clientHeight || 550;
    canvas.width = width * window.devicePixelRatio;
    canvas.height = height * window.devicePixelRatio;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.scale(window.devicePixelRatio, window.devicePixelRatio);
    requestDraw();
  }

  /**
   * Switches active tab view (graph canvas, tabular records, or compiled query view).
   * @param {"graph" | "table" | "query"} id
   */
  function switchTab(id) {
    if (activeTab === id) return;
    activeTab = id;

    tabGraph.className = `voyager-tab-btn ${id === "graph" ? "active" : ""}`;
    tabTable.className = `voyager-tab-btn ${id === "table" ? "active" : ""}`;
    tabQuery.className = `voyager-tab-btn ${id === "query" ? "active" : ""}`;

    canvasContainer.style.display = id === "graph" ? "block" : "none";
    tableContainer.style.display = id === "table" ? "flex" : "none";
    queryContainer.style.display = id === "query" ? "block" : "none";
    toolbar.style.display = id === "graph" ? "flex" : "none";

    if (id === "graph") {
      resize();
      wakeSimulation();
    } else {
      if (rafId) { cancelAnimationFrame(rafId); rafId = null; }
      if (tweenRafId) { cancelAnimationFrame(tweenRafId); tweenRafId = null; }
      isSimulating = false;
      if (statusEl) statusEl.textContent = "Settled";
    }
  }

  /**
   * Global keyboard shortcut handler for canvas operations.
   * @param {KeyboardEvent} evt
   */
  function onKeyDown(evt) {
    if (evt.target instanceof HTMLInputElement || evt.target instanceof HTMLSelectElement) return;
    if (evt.ctrlKey || evt.metaKey || evt.altKey) return;

    if (evt.key === "Escape") {
      closeInspector(true);
    } else if (evt.key === "f" || evt.key === "F") {
      if (activeTab === "graph") fitView();
    } else if (evt.key === "p" || evt.key === "P") {
      if (activeTab === "graph") togglePhysics();
    }
  }
  root.addEventListener("keydown", onKeyDown);

  // Search input filter and zoom-to-match listener
  searchInput.addEventListener("input", (e) => {
    searchTerm = e.target.value.toLowerCase().trim();
    requestDraw();
  });

  searchInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && searchTerm) {
      const match = nodes.find(n => n.label.toLowerCase().includes(searchTerm) || n.id.toLowerCase().includes(searchTerm));
      if (match) {
        transform.k = 1.2;
        transform.x = width / 2 - match.x * transform.k;
        transform.y = height / 2 - match.y * transform.k;
        if (zoomEl) zoomEl.textContent = `${Math.round(transform.k * 100)}%`;
        model.set("selected_node", match.id);
        model.set("selected_edge", "");
        model.save_changes();
        showInspector({ type: "node", obj: match });
        requestDraw();
      }
    }
  });

  fitBtn.addEventListener("click", fitView);

  // HUD zoom control handlers
  hudPill.querySelector(".v-zoom-in").addEventListener("click", () => {
    transform.k = Math.min(4.0, transform.k * 1.2);
    if (zoomEl) zoomEl.textContent = `${Math.round(transform.k * 100)}%`;
    requestDraw();
  });
  hudPill.querySelector(".v-zoom-out").addEventListener("click", () => {
    transform.k = Math.max(0.15, transform.k / 1.2);
    if (zoomEl) zoomEl.textContent = `${Math.round(transform.k * 100)}%`;
    requestDraw();
  });
  hudPill.querySelector(".v-zoom-reset").addEventListener("click", () => {
    let avgX = width / 2, avgY = height / 2;
    if (nodes.length > 0) {
      avgX = nodes.reduce((acc, n) => acc + n.x, 0) / nodes.length;
      avgY = nodes.reduce((acc, n) => acc + n.y, 0) / nodes.length;
    }
    transform.k = 1.0;
    transform.x = width / 2 - avgX;
    transform.y = height / 2 - avgY;
    if (zoomEl) zoomEl.textContent = "100%";
    requestDraw();
  });

  // Opaque PNG snapshot export
  exportPngBtn.addEventListener("click", () => {
    const tempCanvas = document.createElement("canvas");
    tempCanvas.width = canvas.width;
    tempCanvas.height = canvas.height;
    const tempCtx = tempCanvas.getContext("2d");
    tempCtx.fillStyle = theme.bgRoot;
    tempCtx.fillRect(0, 0, tempCanvas.width, tempCanvas.height);
    tempCtx.drawImage(canvas, 0, 0);

    const link = document.createElement("a");
    link.download = `voyager_graph_${Date.now()}.png`;
    link.href = tempCanvas.toDataURL("image/png");
    link.click();
  });

  // Layout switcher listener
  layoutSelect.addEventListener("change", (e) => {
    currentLayout = e.target.value;
    applyLayoutWithTween(currentLayout);
  });

  /**
   * Applies target layout algorithm with cubic easing coordinate interpolation.
   * @param {"force" | "circular" | "hierarchical"} layoutType
   */
  function applyLayoutWithTween(layoutType) {
    if (nodes.length === 0) return;
    if (tweenRafId) { cancelAnimationFrame(tweenRafId); tweenRafId = null; }

    if (layoutType === "circular") {
      isPhysicsPaused = true;
      pauseBtn.innerHTML = `<span>Resume</span>`;
      if (statusEl) statusEl.textContent = "Arranging...";
      const radius = Math.min(width, height) * 0.38;
      const sorted = [...nodes].sort((a, b) => (a.group.localeCompare(b.group) || (b.inDegree + b.outDegree) - (a.inDegree + a.outDegree)));
      sorted.forEach((n, i) => {
        const theta = (i / sorted.length) * 2 * Math.PI - Math.PI / 2;
        n.startX = n.x;
        n.startY = n.y;
        n.targetX = width / 2 + radius * Math.cos(theta);
        n.targetY = height / 2 + radius * Math.sin(theta);
      });
      runTweenAnimation();
    } else if (layoutType === "hierarchical") {
      isPhysicsPaused = true;
      pauseBtn.innerHTML = `<span>Resume</span>`;
      if (statusEl) statusEl.textContent = "Arranging...";
      const layers = new Map();
      nodes.forEach(n => {
        const rank = Math.max(0, n.inDegree - n.outDegree + 2);
        if (!layers.has(rank)) layers.set(rank, []);
        layers.get(rank).push(n);
      });
      const sortedRanks = [...layers.keys()].sort((a, b) => a - b);
      const layerSpacing = height / (sortedRanks.length + 1);

      sortedRanks.forEach((rank, rIdx) => {
        const rowNodes = layers.get(rank);
        const colSpacing = width / (rowNodes.length + 1);
        rowNodes.forEach((n, cIdx) => {
          n.startX = n.x;
          n.startY = n.y;
          n.targetX = colSpacing * (cIdx + 1);
          n.targetY = layerSpacing * (rIdx + 1);
        });
      });
      runTweenAnimation();
    } else if (layoutType === "force") {
      isPhysicsPaused = false;
      pauseBtn.innerHTML = `<span>Pause</span>`;
      wakeSimulation();
    }
  }

  /**
   * Executes coordinate tween step animation.
   */
  function runTweenAnimation() {
    let startTime = performance.now();
    const duration = 400;

    function stepTween(now) {
      const elapsed = now - startTime;
      const progress = Math.min(1.0, elapsed / duration);
      const ease = 1 - Math.pow(1 - progress, 3);

      nodes.forEach(n => {
        if (n.targetX !== null && n.targetY !== null) {
          n.x = n.startX + (n.targetX - n.startX) * ease;
          n.y = n.startY + (n.targetY - n.startY) * ease;
          n.vx = 0;
          n.vy = 0;
        }
      });

      draw();

      if (progress < 1.0) {
        tweenRafId = requestAnimationFrame(stepTween);
      } else {
        tweenRafId = null;
        if (statusEl) statusEl.textContent = "Settled";
        fitView();
      }
    }
    tweenRafId = requestAnimationFrame(stepTween);
  }

  /**
   * Advances the force-directed simulation step using Verlet integration,
   * repulsive node charges, spring edge attractions, and center gravitational damping.
   */
  function stepSimulation() {
    if (isPhysicsPaused || currentLayout !== "force") {
      isSimulating = false;
      if (statusEl) statusEl.textContent = isPhysicsPaused ? "Paused" : "Settled";
      return;
    }

    const kRepel = 2400 * simAlpha;
    const kAttract = 0.045;
    const damping = 0.78;

    for (let i = 0; i < nodes.length; i++) {
      for (let j = i + 1; j < nodes.length; j++) {
        let dx = nodes[j].x - nodes[i].x;
        let dy = nodes[j].y - nodes[i].y;
        if (dx === 0 && dy === 0) {
          dx = (Math.random() - 0.5) * 2;
          dy = (Math.random() - 0.5) * 2;
        }
        const distSq = Math.max(dx * dx + dy * dy, 25.0);
        const dist = Math.sqrt(distSq);
        if (dist < 400) {
          const force = kRepel / distSq;
          const fx = (dx / dist) * force;
          const fy = (dy / dist) * force;
          nodes[i].vx -= fx;
          nodes[i].vy -= fy;
          nodes[j].vx += fx;
          nodes[j].vy += fy;
        }
      }
    }

    edges.forEach((e) => {
      if (e.source === e.target) return;
      const dx = e.target.x - e.source.x;
      const dy = e.target.y - e.source.y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 1;
      const force = (dist - 110) * kAttract * simAlpha;
      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;
      e.source.vx += fx;
      e.source.vy += fy;
      e.target.vx -= fx;
      e.target.vy -= fy;
    });

    let totalKineticEnergy = 0;
    nodes.forEach((n) => {
      if (n !== draggedNode) {
        n.vx += (width / 2 - n.x) * 0.002 * simAlpha;
        n.vy += (height / 2 - n.y) * 0.002 * simAlpha;
        n.vx = Math.max(-35, Math.min(35, n.vx));
        n.vy = Math.max(-35, Math.min(35, n.vy));
        n.x += n.vx;
        n.y += n.vy;
        if (!Number.isFinite(n.x)) n.x = width / 2;
        if (!Number.isFinite(n.y)) n.y = height / 2;
        n.vx *= damping;
        n.vy *= damping;
        totalKineticEnergy += (n.vx * n.vx + n.vy * n.vy);
      }
    });

    simAlpha *= 0.985;
    if (simAlpha < 0.005 || totalKineticEnergy < 0.02) {
      isSimulating = false;
      simAlpha = 0;
      if (statusEl) statusEl.textContent = "Settled";
    }
  }

  /**
   * Draws a level-of-detail Cartesian dot grid aligned with viewport transformations.
   */
  function drawDotGrid() {
    let rawGridSize = 24 * Math.max(0.1, transform.k || 1);
    while (rawGridSize < 14) rawGridSize *= 2;
    while (rawGridSize > 64) rawGridSize /= 2;

    const startX = ((transform.x % rawGridSize) + rawGridSize) % rawGridSize;
    const startY = ((transform.y % rawGridSize) + rawGridSize) % rawGridSize;

    ctx.fillStyle = theme.dotGrid;
    ctx.beginPath();
    for (let x = startX; x < width; x += rawGridSize) {
      for (let y = startY; y < height; y += rawGridSize) {
        ctx.rect(x - 0.75, y - 0.75, 1.5, 1.5);
      }
    }
    ctx.fill();
  }

  /**
   * Computes quadratic Bezier control points for straight, curved multi-edges, or self-loops.
   * @param {Object} e - Edge descriptor.
   * @returns {{cx: number, cy: number}}
   */
  function getEdgeControlPoint(e) {
    if (e.source === e.target) {
      return { cx: e.source.x, cy: e.source.y - e.source.size - 22 };
    }
    const midX = (e.source.x + e.target.x) / 2;
    const midY = (e.source.y + e.target.y) / 2;
    if (e.pairIndex === 0) {
      return { cx: midX, cy: midY };
    }
    let dx = e.target.x - e.source.x;
    let dy = e.target.y - e.source.y;
    let len = Math.sqrt(dx * dx + dy * dy) || 1;
    let nx = -dy / len;
    let ny = dx / len;

    if (e.isReverse) {
      nx = -nx;
      ny = -ny;
    }

    const curvature = (e.pairIndex % 2 === 1 ? 1 : -1) * Math.ceil(e.pairIndex / 2) * 24;
    return {
      cx: midX + nx * curvature,
      cy: midY + ny * curvature,
    };
  }

  /**
   * Draws a directed arrowhead on the canvas along the terminal edge tangent.
   */
  function drawArrowhead(fromX, fromY, toX, toY, radius, color) {
    const dx = toX - fromX;
    const dy = toY - fromY;
    const len = Math.sqrt(dx * dx + dy * dy) || 1;
    const nx = dx / len;
    const ny = dy / len;
    const targetX = toX - nx * (radius + 2);
    const targetY = toY - ny * (radius + 2);
    const arrowLen = 9;
    const arrowWidth = 5;

    ctx.save();
    ctx.beginPath();
    ctx.moveTo(targetX, targetY);
    ctx.lineTo(
      targetX - nx * arrowLen - ny * arrowWidth,
      targetY - ny * arrowLen + nx * arrowWidth
    );
    ctx.lineTo(
      targetX - nx * arrowLen + ny * arrowWidth,
      targetY - ny * arrowLen - nx * arrowWidth
    );
    ctx.closePath();
    ctx.fillStyle = color;
    ctx.fill();
    ctx.restore();
  }

  /**
   * Main canvas render pass for graph nodes, edges, labels, halos, and selection highlights.
   */
  function draw() {
    if (activeTab !== "graph") return;

    ctx.save();
    ctx.clearRect(0, 0, width, height);

    if (nodes.length === 0) {
      ctx.fillStyle = theme.textMuted;
      ctx.font = "13px sans-serif";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText("No graph nodes or edges to display.", width / 2, height / 2 - 10);
      ctx.font = "11px sans-serif";
      ctx.fillText("Switch to the Table or Query tab to inspect records.", width / 2, height / 2 + 12);
      ctx.restore();
      return;
    }

    drawDotGrid();

    ctx.translate(transform.x, transform.y);
    ctx.scale(transform.k, transform.k);

    const activeNode = (selectedEntity && selectedEntity.type === "node") ? selectedEntity.obj : null;
    const selectedEdgeId = String(model.get("selected_edge") || "");

    // Render relationship edges
    edges.forEach((e) => {
      const isFiltered = activeLabelFilters.has(e.source.group) || activeLabelFilters.has(e.target.group);
      const isDimmed = (activeNode && (e.source !== activeNode && e.target !== activeNode)) || (searchTerm && !e.source.label.toLowerCase().includes(searchTerm) && !e.target.label.toLowerCase().includes(searchTerm));
      const isSelected = (selectedEntity && selectedEntity.type === "edge" && selectedEntity.obj === e) || e.id === selectedEdgeId;
      const isHovered = hoveredEntity && hoveredEntity.type === "edge" && hoveredEntity.obj === e;

      const edgeColor = isSelected ? theme.selection : (isHovered ? theme.accent : (isDimmed || isFiltered ? (isLight ? "#e2e8f0" : "#1e293b") : e.color));

      ctx.beginPath();
      if (e.source === e.target) {
        const loopR = 18;
        ctx.arc(e.source.x, e.source.y - e.source.size - loopR, loopR, 0, 2 * Math.PI);
      } else {
        const { cx, cy } = getEdgeControlPoint(e);
        ctx.moveTo(e.source.x, e.source.y);
        ctx.quadraticCurveTo(cx, cy, e.target.x, e.target.y);
      }
      ctx.strokeStyle = edgeColor;
      ctx.lineWidth = isSelected || isHovered ? 2.5 : 1.5;
      ctx.stroke();

      if (e.source === e.target) {
        drawArrowhead(e.source.x - 5, e.source.y - e.source.size - 4, e.source.x, e.source.y, e.source.size, edgeColor);
      } else {
        const { cx, cy } = getEdgeControlPoint(e);
        drawArrowhead(cx, cy, e.target.x, e.target.y, e.target.size, edgeColor);
      }

      if (e.label && !isDimmed && !isFiltered) {
        const { cx, cy } = getEdgeControlPoint(e);
        ctx.font = "10px ui-monospace, monospace";
        const textWidth = ctx.measureText(e.label).width;

        ctx.fillStyle = theme.bgNav;
        ctx.fillRect(cx - textWidth / 2 - 4, cy - 7, textWidth + 8, 14);
        ctx.fillStyle = isSelected ? theme.selection : (isHovered ? theme.accent : theme.textMuted);
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(e.label, cx, cy);
      }
    });

    // Render graph nodes
    const selectedNodeId = String(model.get("selected_node") || "");

    nodes.forEach((n) => {
      const isFiltered = activeLabelFilters.has(n.group);
      const matchesSearch = searchTerm && (n.label.toLowerCase().includes(searchTerm) || n.id.toLowerCase().includes(searchTerm));
      const isDimmed = (activeNode && (n !== activeNode && !activeNode.neighbors.has(n.id))) || (searchTerm && !matchesSearch);
      const isSelected = (selectedEntity && selectedEntity.type === "node" && selectedEntity.obj === n) || n.id === selectedNodeId;
      const isHovered = hoveredEntity && hoveredEntity.type === "node" && hoveredEntity.obj === n;

      if (isDimmed || isFiltered) {
        ctx.beginPath();
        ctx.arc(n.x, n.y, n.size, 0, 2 * Math.PI);
        ctx.fillStyle = isLight ? "#cbd5e1" : "#1e293b";
        ctx.fill();
        return;
      }

      // Selection and search glow halo
      if (isSelected || matchesSearch || isHovered) {
        ctx.beginPath();
        ctx.arc(n.x, n.y, n.size + (isSelected ? 8 : (isHovered ? 6 : 5)), 0, 2 * Math.PI);
        ctx.fillStyle = isSelected ? "rgba(244, 63, 94, 0.28)" : (isHovered ? theme.accentGlow : "rgba(56, 189, 248, 0.2)");
        ctx.fill();
      }

      ctx.beginPath();
      ctx.arc(n.x, n.y, n.size + (isHovered ? 2 : 0), 0, 2 * Math.PI);
      ctx.fillStyle = isSelected ? theme.selection : n.color;
      ctx.fill();
      ctx.lineWidth = isSelected ? 2.5 : 1.5;
      ctx.strokeStyle = isSelected ? "#ffffff" : theme.bgNav;
      ctx.stroke();

      ctx.font = isSelected ? "bold 12px sans-serif" : "11px sans-serif";
      const displayLabel = n.label.length > 28 ? n.label.slice(0, 26) + "..." : n.label;
      const lblWidth = ctx.measureText(displayLabel).width;
      ctx.fillStyle = isLight ? "rgba(255, 255, 255, 0.9)" : "rgba(13, 19, 31, 0.9)";
      ctx.fillRect(n.x - lblWidth / 2 - 4, n.y + n.size + 3, lblWidth + 8, 16);

      ctx.fillStyle = isSelected ? theme.selection : theme.textPrimary;
      ctx.textAlign = "center";
      ctx.textBaseline = "top";
      ctx.fillText(displayLabel, n.x, n.y + n.size + 5);
    });

    ctx.restore();
  }

  /**
   * Animation frame loop driver for active simulations.
   */
  function runLoop() {
    if (isSimulating) {
      stepSimulation();
      draw();
      rafId = requestAnimationFrame(runLoop);
    }
  }

  /**
   * Transforms raw client coordinates to canvas world coordinates.
   */
  function getMousePos(evt) {
    const rect = canvas.getBoundingClientRect();
    const clientX = evt.clientX - rect.left;
    const clientY = evt.clientY - rect.top;
    return {
      x: (clientX - transform.x) / transform.k,
      y: (clientY - transform.y) / transform.k,
      rawX: clientX,
      rawY: clientY,
    };
  }

  /**
   * Performs hit-testing to identify the topmost node at the given world position.
   */
  function findNode(pos) {
    const hitRadiusScreen = 10;
    const hitRadiusWorld = hitRadiusScreen / transform.k;
    for (let i = nodes.length - 1; i >= 0; i--) {
      const n = nodes[i];
      if (activeLabelFilters.has(n.group)) continue;
      const dx = n.x - pos.x;
      const dy = n.y - pos.y;
      const r = n.size + hitRadiusWorld;
      if (dx * dx + dy * dy <= r * r) {
        return n;
      }
    }
    return null;
  }

  /**
   * Performs curve point sampling to identify the closest edge at the given world position.
   */
  function findEdge(pos) {
    const hitThresholdSq = (10 / transform.k) ** 2;
    for (let i = edges.length - 1; i >= 0; i--) {
      const e = edges[i];
      if (activeLabelFilters.has(e.source.group) || activeLabelFilters.has(e.target.group)) continue;
      if (e.source === e.target) {
        const loopCenterY = e.source.y - e.source.size - 18;
        const dx = pos.x - e.source.x;
        const dy = pos.y - loopCenterY;
        if (Math.abs(Math.sqrt(dx * dx + dy * dy) - 18) <= (8 / transform.k)) return e;
      } else {
        const { cx, cy } = getEdgeControlPoint(e);
        for (let t = 0; t <= 1.0; t += 0.04) {
          const px = (1 - t) * (1 - t) * e.source.x + 2 * (1 - t) * t * cx + t * t * e.target.x;
          const py = (1 - t) * (1 - t) * e.source.y + 2 * (1 - t) * t * cy + t * t * e.target.y;
          if ((pos.x - px) ** 2 + (pos.y - py) ** 2 <= hitThresholdSq) {
            return e;
          }
        }
      }
    }
    return null;
  }

  /**
   * Copies text to clipboard and provides animated confirmation on the button element.
   * @param {string} text
   * @param {HTMLElement} btnElement
   * @param {string} defaultHtml
   */
  function copyTextWithMorph(text, btnElement, defaultHtml = `${icons.copy} <span>Copy</span>`) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(() => {
        btnElement.classList.add("voyager-copy-morph", "success");
        btnElement.innerHTML = `${icons.check} <span style="color:${theme.success}; font-weight:600;">Copied</span>`;
        setTimeout(() => {
          btnElement.classList.remove("success");
          btnElement.innerHTML = defaultHtml;
        }, 1400);
      }).catch(() => {
        btnElement.innerHTML = `<span>Error</span>`;
      });
    }
  }

  /**
   * Opens and populates the floating property inspector drawer for a selected entity.
   * @param {{type: "node" | "edge", obj: Object}} entity
   */
  function showInspector(entity) {
    selectedEntity = entity;
    inspector.innerHTML = "";
    inspector.style.display = "flex";
    inspector.style.opacity = "0";
    inspector.style.transform = "translateX(20px)";

    requestAnimationFrame(() => {
      inspector.style.opacity = "1";
      inspector.style.transform = "translateX(0)";
    });

    if (width < 720 && isLeftPanelOpen) {
      setLeftPanelOpen(false);
    }

    const topRow = document.createElement("div");
    topRow.style.display = "flex";
    topRow.style.justifyContent = "space-between";
    topRow.style.alignItems = "center";
    topRow.style.marginBottom = "10px";

    const titleBadge = document.createElement("div");
    titleBadge.className = "voyager-mono";
    titleBadge.style.fontSize = "13px";
    titleBadge.style.fontWeight = "600";

    const rightControls = document.createElement("div");
    rightControls.style.display = "flex";
    rightControls.style.alignItems = "center";
    rightControls.style.gap = "6px";

    const copyJsonBtn = document.createElement("button");
    copyJsonBtn.className = "voyager-btn";
    copyJsonBtn.innerHTML = `${icons.copy} <span>JSON</span>`;
    copyJsonBtn.style.padding = "4px 8px";
    copyJsonBtn.style.fontSize = "11px";

    const closeBtn = document.createElement("button");
    closeBtn.className = "voyager-btn";
    closeBtn.innerHTML = icons.close;
    closeBtn.style.background = "transparent";
    closeBtn.style.border = "none";
    closeBtn.style.color = theme.textMuted;
    closeBtn.style.padding = "4px";
    closeBtn.addEventListener("click", () => closeInspector(true));

    rightControls.appendChild(copyJsonBtn);
    rightControls.appendChild(closeBtn);
    topRow.appendChild(titleBadge);
    topRow.appendChild(rightControls);
    inspector.appendChild(topRow);

    let entityData = {};

    if (entity.type === "node") {
      const n = entity.obj;
      entityData = { id: n.id, label: n.label, group: n.group, ...n.data };
      titleBadge.textContent = `(:${n.group})`;
      titleBadge.style.color = n.color;

      const subInfo = document.createElement("div");
      subInfo.style.fontSize = "11px";
      subInfo.style.color = theme.textMuted;
      subInfo.style.marginBottom = "12px";
      subInfo.textContent = `ID: ${n.id}  ·  In-degree: ${n.inDegree}  ·  Out-degree: ${n.outDegree}`;
      inspector.appendChild(subInfo);
    } else if (entity.type === "edge") {
      const e = entity.obj;
      entityData = { id: e.id, label: e.label, source: e.source.id, target: e.target.id, ...e.data };
      titleBadge.textContent = `[:${e.label || "CONNECTED_TO"}]`;
      titleBadge.style.color = theme.accent;

      const subInfo = document.createElement("div");
      subInfo.style.fontSize = "11px";
      subInfo.style.color = theme.textMuted;
      subInfo.style.marginBottom = "12px";
      subInfo.textContent = `(${e.source.label}) ➔ (${e.target.label})`;
      inspector.appendChild(subInfo);
    }

    copyJsonBtn.addEventListener("click", () => {
      const safeJson = JSON.stringify(entityData, (k, v) => typeof v === "bigint" ? v.toString() : v, 2);
      copyTextWithMorph(safeJson, copyJsonBtn, `${icons.copy} <span>JSON</span>`);
    });

    const propsList = document.createElement("div");
    propsList.style.fontSize = "12px";

    const dataKeys = Object.keys(entityData);
    if (dataKeys.length === 0) {
      propsList.innerHTML = `<div style="color: ${theme.textMuted};">No properties attached.</div>`;
    } else {
      dataKeys.forEach(k => {
        const row = document.createElement("div");
        row.className = "voyager-prop-row";

        const val = entityData[k];
        const valStr = (val !== null && typeof val === "object")
          ? JSON.stringify(val, (k, v) => typeof v === "bigint" ? v.toString() : v, 2)
          : String(val !== null && val !== undefined ? val : "null");

        const headerDiv = document.createElement("div");
        headerDiv.style.display = "flex";
        headerDiv.style.justifyContent = "space-between";
        headerDiv.style.alignItems = "center";

        const keyTag = document.createElement("strong");
        keyTag.style.color = theme.textPrimary;
        keyTag.className = "voyager-mono";
        keyTag.style.fontSize = "11px";
        keyTag.textContent = k;

        const copyBtn = document.createElement("button");
        copyBtn.className = "voyager-btn";
        copyBtn.style.fontSize = "10px";
        copyBtn.style.background = "transparent";
        copyBtn.style.border = "none";
        copyBtn.style.color = theme.textMuted;
        copyBtn.style.padding = "2px 4px";
        copyBtn.innerHTML = `${icons.copy}`;
        copyBtn.addEventListener("click", (e) => {
          e.stopPropagation();
          copyTextWithMorph(valStr, copyBtn, icons.copy);
        });

        headerDiv.appendChild(keyTag);
        headerDiv.appendChild(copyBtn);

        const valPre = document.createElement("pre");
        valPre.style.margin = "4px 0 0 0";
        valPre.style.whiteSpace = "pre-wrap";
        valPre.style.wordBreak = "break-word";
        valPre.style.fontSize = "11px";
        valPre.style.color = theme.accent;
        valPre.className = "voyager-mono";
        valPre.textContent = valStr;

        row.appendChild(headerDiv);
        row.appendChild(valPre);
        propsList.appendChild(row);
      });
    }
    inspector.appendChild(propsList);
    requestDraw();
  }

  // Canvas mouse event listeners for selection, drag, and pan
  canvas.addEventListener("mousedown", (evt) => {
    if (evt.button !== 0) return;
    const pos = getMousePos(evt);
    dragDisplacement = 0;

    const hitNode = findNode(pos);
    if (hitNode) {
      draggedNode = hitNode;
      model.set("selected_node", hitNode.id);
      model.set("selected_edge", "");
      model.save_changes();
      showInspector({ type: "node", obj: hitNode });
      wakeSimulation();
      return;
    }

    const hitEdge = findEdge(pos);
    if (hitEdge) {
      model.set("selected_node", "");
      model.set("selected_edge", hitEdge.id);
      model.save_changes();
      showInspector({ type: "edge", obj: hitEdge });
      requestDraw();
      return;
    }

    isPanning = true;
    panStart = { x: evt.clientX - transform.x, y: evt.clientY - transform.y };
  });

  function onMouseMove(evt) {
    const pos = getMousePos(evt);

    if (draggedNode) {
      dragDisplacement += 1;
      draggedNode.x = pos.x;
      draggedNode.y = pos.y;
      draggedNode.vx = 0;
      draggedNode.vy = 0;
      wakeSimulation();
    } else if (isPanning) {
      dragDisplacement += 1;
      transform.x = evt.clientX - panStart.x;
      transform.y = evt.clientY - panStart.y;
      requestDraw();
    } else {
      if (pos.rawX < 0 || pos.rawX > width || pos.rawY < 0 || pos.rawY > height || evt.target !== canvas) {
        if (hoveredEntity !== null) {
          hoveredEntity = null;
          canvas.style.cursor = "default";
          tooltip.style.display = "none";
          requestDraw();
        }
        return;
      }

      const hNode = findNode(pos);
      if (hNode) {
        if (hoveredEntity?.obj !== hNode) {
          hoveredEntity = { type: "node", obj: hNode };
          canvas.style.cursor = "pointer";
          tooltip.style.display = "block";
          tooltip.style.left = `${Math.max(12, Math.min(pos.rawX + 12, width - 160))}px`;
          tooltip.style.top = `${Math.max(12, Math.min(pos.rawY + 12, height - 60))}px`;
          tooltip.textContent = `${hNode.label} (${hNode.group})`;
          requestDraw();
        }
        return;
      }

      const hEdge = findEdge(pos);
      if (hEdge) {
        if (hoveredEntity?.obj !== hEdge) {
          hoveredEntity = { type: "edge", obj: hEdge };
          canvas.style.cursor = "pointer";
          tooltip.style.display = "block";
          tooltip.style.left = `${Math.max(12, Math.min(pos.rawX + 12, width - 200))}px`;
          tooltip.style.top = `${Math.max(12, Math.min(pos.rawY + 12, height - 60))}px`;
          tooltip.textContent = `[:${hEdge.label || "EDGE"}] ${hEdge.source.label} ➔ ${hEdge.target.label}`;
          requestDraw();
        }
        return;
      }

      if (hoveredEntity !== null) {
        hoveredEntity = null;
        canvas.style.cursor = "default";
        tooltip.style.display = "none";
        requestDraw();
      }
    }
  }
  window.addEventListener("mousemove", onMouseMove);

  canvas.addEventListener("mouseleave", () => {
    if (hoveredEntity !== null) {
      hoveredEntity = null;
      canvas.style.cursor = "default";
      tooltip.style.display = "none";
      requestDraw();
    }
  });

  function onMouseUp() {
    if (isPanning && dragDisplacement < 4 && selectedEntity) {
      closeInspector(true);
    }
    draggedNode = null;
    isPanning = false;
  }
  window.addEventListener("mouseup", onMouseUp);

  canvas.addEventListener("wheel", (evt) => {
    evt.preventDefault();
    const factor = Math.min(Math.max(Math.exp(-evt.deltaY * 0.002), 0.8), 1.25);
    const newK = Math.max(0.15, Math.min(4.0, transform.k * factor));
    const pos = getMousePos(evt);
    transform.x = pos.rawX - (pos.rawX - transform.x) * (newK / transform.k);
    transform.y = pos.rawY - (pos.rawY - transform.y) * (newK / transform.k);
    transform.k = newK;
    if (zoomEl) zoomEl.textContent = `${Math.round(transform.k * 100)}%`;
    requestDraw();
  });

  /**
   * Initializes tabular records view with tri-state column sorting and client-side pagination.
   */
  function initTableView() {
    tableContainer.innerHTML = "";
    if (!rawRecords || rawRecords.length === 0) {
      tableContainer.innerHTML = `<div style="color: ${theme.textMuted}; padding: 30px; text-align: center;">No tabular records available.</div>`;
      return;
    }

    const headerSet = new Set();
    rawRecords.forEach(r => {
      if (r && typeof r === "object") {
        Object.keys(r).forEach(k => headerSet.add(k));
      }
    });
    const headers = Array.from(headerSet);

    let sortCol = "";
    let sortState = 0; // 0: None, 1: Asc, 2: Desc
    let pageIndex = 0;
    const pageSize = 50;
    let currentFiltered = [...rawRecords];

    const tableTopBar = document.createElement("div");
    tableTopBar.style.display = "flex";
    tableTopBar.style.justifyContent = "space-between";
    tableTopBar.style.alignItems = "center";
    tableTopBar.style.padding = "10px 16px";
    tableTopBar.style.borderBottom = `1px solid ${theme.borderSubtle}`;
    tableTopBar.style.background = theme.bgNav;

    const searchWrapper = document.createElement("div");
    searchWrapper.className = "voyager-search-wrap";
    searchWrapper.style.gap = "8px";

    const tableSearchIcon = document.createElement("span");
    tableSearchIcon.className = "voyager-search-icon";
    tableSearchIcon.innerHTML = icons.search;
    searchWrapper.appendChild(tableSearchIcon);

    const tableSearch = document.createElement("input");
    tableSearch.type = "text";
    tableSearch.className = "voyager-input";
    tableSearch.placeholder = "Filter rows...";
    tableSearch.style.paddingLeft = "26px";
    tableSearch.style.width = "200px";

    const rowCountText = document.createElement("div");
    rowCountText.style.fontSize = "11px";
    rowCountText.style.color = theme.textMuted;
    rowCountText.style.marginLeft = "8px";

    searchWrapper.appendChild(tableSearch);
    searchWrapper.appendChild(rowCountText);
    tableTopBar.appendChild(searchWrapper);

    const rightControls = document.createElement("div");
    rightControls.style.display = "flex";
    rightControls.style.alignItems = "center";
    rightControls.style.gap = "8px";

    const pageInfo = document.createElement("span");
    pageInfo.style.fontSize = "11px";
    pageInfo.style.color = theme.textMuted;

    const prevBtn = document.createElement("button");
    prevBtn.className = "voyager-btn";
    prevBtn.innerHTML = icons.chevronLeft;
    prevBtn.style.padding = "4px 8px";
    prevBtn.style.minHeight = "24px";

    const nextBtn = document.createElement("button");
    nextBtn.className = "voyager-btn";
    nextBtn.innerHTML = icons.chevronRight;
    nextBtn.style.padding = "4px 8px";
    nextBtn.style.minHeight = "24px";

    const copyCsvBtn = document.createElement("button");
    copyCsvBtn.className = "voyager-btn";
    copyCsvBtn.innerHTML = `${icons.copy} <span>CSV</span>`;
    copyCsvBtn.style.fontSize = "11px";

    copyCsvBtn.addEventListener("click", () => {
      const csvLines = [headers.map(h => `"${h.replace(/"/g, '""')}"`).join(",")];
      currentFiltered.forEach(r => {
        csvLines.push(headers.map(h => {
          const val = r[h];
          if (val === null || val === undefined) return "";
          const strVal = typeof val === "object" ? JSON.stringify(val) : String(val);
          return `"${strVal.replace(/"/g, '""')}"`;
        }).join(","));
      });
      copyTextWithMorph(csvLines.join("\r\n"), copyCsvBtn, `${icons.copy} <span>CSV</span>`);
    });

    rightControls.appendChild(prevBtn);
    rightControls.appendChild(pageInfo);
    rightControls.appendChild(nextBtn);
    rightControls.appendChild(copyCsvBtn);
    tableTopBar.appendChild(rightControls);
    tableContainer.appendChild(tableTopBar);

    const tableScrollWrap = document.createElement("div");
    tableScrollWrap.style.flex = "1";
    tableScrollWrap.style.overflow = "auto";
    tableContainer.appendChild(tableScrollWrap);

    const table = document.createElement("table");
    table.style.width = "100%";
    table.style.borderCollapse = "separate";
    table.style.borderSpacing = "0";
    table.style.fontSize = "12px";
    table.style.color = theme.textPrimary;

    const thead = document.createElement("thead");
    const trH = document.createElement("tr");

    const headerThMap = new Map();

    headers.forEach(h => {
      const th = document.createElement("th");
      th.style.position = "sticky";
      th.style.top = "0";
      th.style.zIndex = "2";
      th.style.textAlign = "left";
      th.style.padding = "8px 12px";
      th.style.borderBottom = `2px solid ${theme.borderStrong}`;
      th.style.background = theme.bgNav;
      th.style.whiteSpace = "nowrap";
      th.style.cursor = "pointer";

      const colType = rawTypes[h] || (typeof rawRecords[0]?.[h] === "number" ? "i64" : "str");
      th.innerHTML = `<span>${escapeHtml(h)}</span> <span class="v-sort-indicator" style="font-size:10px; color:${theme.textMuted};"></span> <span style="font-size: 10px; font-weight: 600; color: ${theme.accent}; background: ${theme.bgSubtle}; padding: 1px 5px; border-radius: 4px; margin-left: 4px;">${escapeHtml(colType)}</span>`;

      th.addEventListener("click", () => {
        if (sortCol === h) {
          sortState = (sortState + 1) % 3;
          if (sortState === 0) sortCol = "";
        } else {
          sortCol = h;
          sortState = 1;
        }
        updateSortIndicators();
        renderTableRows(tableSearch.value);
      });

      headerThMap.set(h, th);
      trH.appendChild(th);
    });
    thead.appendChild(trH);
    table.appendChild(thead);

    function updateSortIndicators() {
      headerThMap.forEach((th, h) => {
        const ind = th.querySelector(".v-sort-indicator");
        if (ind) {
          if (sortCol === h && sortState === 1) ind.textContent = " ▲";
          else if (sortCol === h && sortState === 2) ind.textContent = " ▼";
          else ind.textContent = "";
        }
      });
    }

    const tbody = document.createElement("tbody");

    function renderTableRows(filterTerm = "") {
      tbody.innerHTML = "";
      const lower = filterTerm.toLowerCase().trim();

      const filtered = rawRecords.filter(row => {
        if (!lower) return true;
        return Object.values(row).some(v => {
          if (v === null || v === undefined) return false;
          const str = typeof v === "object" ? JSON.stringify(v) : String(v);
          return str.toLowerCase().includes(lower);
        });
      });

      if (sortCol && sortState > 0) {
        filtered.sort((a, b) => {
          const vA = a[sortCol];
          const vB = b[sortCol];
          if (vA === vB) return 0;
          if (vA === null || vA === undefined) return 1;
          if (vB === null || vB === undefined) return -1;
          return (vA > vB ? 1 : -1) * (sortState === 1 ? 1 : -1);
        });
      }

      currentFiltered = filtered;

      if (filtered.length === 0) {
        const emptyTr = document.createElement("tr");
        const emptyTd = document.createElement("td");
        emptyTd.colSpan = headers.length || 1;
        emptyTd.style.textAlign = "center";
        emptyTd.style.padding = "32px";
        emptyTd.style.color = theme.textMuted;
        emptyTd.textContent = "No records match your filter.";
        emptyTr.appendChild(emptyTd);
        tbody.appendChild(emptyTr);
        rowCountText.textContent = `0 of ${rawRecords.length} rows`;
        pageInfo.textContent = "Page 0 of 0";
        prevBtn.disabled = true;
        nextBtn.disabled = true;
        return;
      }

      const totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
      pageIndex = Math.min(pageIndex, totalPages - 1);

      pageInfo.textContent = `Page ${pageIndex + 1} of ${totalPages}`;
      rowCountText.textContent = `${filtered.length} of ${rawRecords.length} rows`;
      prevBtn.disabled = pageIndex === 0;
      nextBtn.disabled = pageIndex >= totalPages - 1;

      const pageRows = filtered.slice(pageIndex * pageSize, (pageIndex + 1) * pageSize);

      pageRows.forEach((row, i) => {
        const tr = document.createElement("tr");
        tr.className = "voyager-table-row";
        tr.style.background = i % 2 === 0 ? "transparent" : (isLight ? "#f8fafc" : "#0d131f");

        headers.forEach(h => {
          const td = document.createElement("td");
          const val = row[h];
          if (val === null || val === undefined) {
            td.innerHTML = `<span style="color:${theme.textMuted}; font-style:italic;">null</span>`;
          } else if (typeof val === "object") {
            td.textContent = JSON.stringify(val);
          } else {
            td.textContent = String(val);
          }
          td.style.padding = "6px 12px";
          td.style.borderBottom = `1px solid ${theme.borderSubtle}`;
          td.style.whiteSpace = "nowrap";
          tr.appendChild(td);
        });
        tbody.appendChild(tr);
      });
    }

    prevBtn.addEventListener("click", () => {
      if (pageIndex > 0) {
        pageIndex -= 1;
        renderTableRows(tableSearch.value);
      }
    });

    nextBtn.addEventListener("click", () => {
      pageIndex += 1;
      renderTableRows(tableSearch.value);
    });

    renderTableRows();
    tableSearch.addEventListener("input", (e) => {
      pageIndex = 0;
      renderTableRows(e.target.value);
    });

    table.appendChild(tbody);
    tableScrollWrap.appendChild(table);
  }

  /**
   * Applies syntax highlighting to Cypher, ISO GQL, and PGQ statements.
   * @param {string} code
   * @returns {string} Safe HTML string with syntax token classes.
   */
  function highlightDialect(code) {
    const stringPlaceholders = [];
    let strClean = String(code).replace(/("[^"\\]*(?:\\.[^"\\]*)*"|'[^'\\]*(?:\\.[^'\\]*)*')/g, (match) => {
      const idx = stringPlaceholders.length;
      stringPlaceholders.push(match);
      return `___STR_TOKEN_${idx}___`;
    });

    let safe = escapeHtml(strClean);
    safe = safe.replace(/\b(MATCH|OPTIONAL MATCH|WHERE|RETURN|WITH|CREATE|MERGE|SET|DELETE|DETACH DELETE|REMOVE|UNWIND|CALL|YIELD|ORDER BY|SKIP|LIMIT|GRAPH_TABLE|COLUMNS|AND|OR|NOT|IN|DISTINCT|AS|IS NULL|IS NOT NULL)\b/g, '<span class="voyager-syntax-kw">$1</span>');
    safe = safe.replace(/\[(:[A-Za-z0-9_]+)\]/g, '[<span class="voyager-syntax-rel">$1</span>]');
    safe = safe.replace(/(?<!<span[^>]*)(\b:[A-Za-z0-9_]+)(?![^<]*<\/span>)/g, '<span class="voyager-syntax-lbl">$1</span>');
    safe = safe.replace(/(\$[A-Za-z0-9_]+|%s)/g, '<span class="voyager-syntax-param">$1</span>');

    stringPlaceholders.forEach((origStr, idx) => {
      safe = safe.replace(`___STR_TOKEN_${idx}___`, `<span class="voyager-syntax-str">${escapeHtml(origStr)}</span>`);
    });

    return safe;
  }

  /**
   * Initializes the multi-dialect compiled query view tab.
   */
  function initQueryView() {
    const cypherCode = model.get("query_statement") || "MATCH (n)-[r]->(m) RETURN n, r, m";
    const gqlCode = model.get("gql_statement") || cypherCode;
    const pgqCode = model.get("pgq_statement") || cypherCode;

    queryContainer.innerHTML = "";

    const headerBox = document.createElement("div");
    headerBox.style.display = "flex";
    headerBox.style.justifyContent = "space-between";
    headerBox.style.alignItems = "center";
    headerBox.style.marginBottom = "16px";

    const title = document.createElement("div");
    title.innerHTML = "<strong>Multi-Dialect Compiled Graph Statements</strong>";
    title.style.fontSize = "13px";
    headerBox.appendChild(title);

    queryContainer.appendChild(headerBox);

    function createCodeBox(dialectName, codeStr) {
      const box = document.createElement("div");
      box.style.marginBottom = "20px";

      const bar = document.createElement("div");
      bar.style.display = "flex";
      bar.style.justifyContent = "space-between";
      bar.style.alignItems = "center";
      bar.style.marginBottom = "6px";

      const dTitle = document.createElement("div");
      dTitle.style.fontSize = "11px";
      dTitle.style.color = theme.accent;
      dTitle.style.fontWeight = "600";
      dTitle.textContent = dialectName;
      bar.appendChild(dTitle);

      const copyBtn = document.createElement("button");
      copyBtn.className = "voyager-btn";
      copyBtn.innerHTML = `${icons.copy} <span>Copy</span>`;
      copyBtn.style.padding = "3px 8px";
      copyBtn.style.fontSize = "11px";
      copyBtn.addEventListener("click", () => copyTextWithMorph(codeStr, copyBtn, `${icons.copy} <span>Copy</span>`));
      bar.appendChild(copyBtn);
      box.appendChild(bar);

      const pre = document.createElement("pre");
      pre.className = "voyager-code-block voyager-mono";
      pre.innerHTML = highlightDialect(codeStr);
      box.appendChild(pre);

      return box;
    }

    queryContainer.appendChild(createCodeBox("openCypher 9 / Cypher 25", cypherCode));
    if (gqlCode !== cypherCode) {
      queryContainer.appendChild(createCodeBox("ISO GQL (Graph Query Language)", gqlCode));
    }
    if (pgqCode !== cypherCode) {
      queryContainer.appendChild(createCodeBox("SQL:2023 PGQ / DuckPGQ", pgqCode));
    }
  }

  // Initial layout and render trigger
  window.addEventListener("resize", resize);
  resize();
  initTableView();
  initQueryView();
  wakeSimulation();
  fitView();

  // Traitlet reactivity listeners for external selection changes
  const onSelectedNodeChange = () => {
    const selId = model.get("selected_node");
    if (selId) {
      const target = nodeMap.get(String(selId));
      if (target && (selectedEntity?.type !== "node" || selectedEntity?.obj?.id !== target.id)) {
        showInspector({ type: "node", obj: target });
      }
    } else if (!model.get("selected_edge") && selectedEntity) {
      closeInspector(false);
    }
    requestDraw();
  };
  model.on("change:selected_node", onSelectedNodeChange);

  const onSelectedEdgeChange = () => {
    const selEdgeId = model.get("selected_edge");
    if (selEdgeId) {
      const target = edges.find(e => e.id === selEdgeId);
      if (target && (selectedEntity?.type !== "edge" || selectedEntity?.obj?.id !== target.id)) {
        showInspector({ type: "edge", obj: target });
      }
    } else if (!model.get("selected_node") && selectedEntity) {
      closeInspector(false);
    }
    requestDraw();
  };
  model.on("change:selected_edge", onSelectedEdgeChange);

  /**
   * Cleans up all DOM event listeners, model subscriptions, and animation frames on unmount.
   */
  return () => {
    window.removeEventListener("resize", resize);
    window.removeEventListener("mousemove", onMouseMove);
    window.removeEventListener("mouseup", onMouseUp);
    root.removeEventListener("keydown", onKeyDown);
    model.off("change:selected_node", onSelectedNodeChange);
    model.off("change:selected_edge", onSelectedEdgeChange);
    if (rafId) cancelAnimationFrame(rafId);
    if (tweenRafId) cancelAnimationFrame(tweenRafId);
  };
}
