(function () {
  const TAURI_FALLBACK_BANNER = "Tauri API unavailable — running in web fallback mode";
  const invoke =
    (window.__TAURI__ &&
      ((window.__TAURI__.core && window.__TAURI__.core.invoke) ||
        (window.__TAURI__.tauri && window.__TAURI__.tauri.invoke))) ||
    window.__TAURI_INVOKE__ ||
    (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
  const DEMO_TOUR_SEQUENCE = ["overview", "files", "claims", "drift", "decision"];
  const DEMO_TOUR_STEP_MS_DEFAULT = 1000;
  const DEMO_TOUR_AUTOSTART_DELAY_MS = 500;
  const DEMO_HOTKEY_TABS = {
    1: "overview",
    2: "files",
    3: "claims",
    4: "drift",
    5: "decision",
  };

  const state = {
    pack: null,
    selectedClaimId: null,
    selectedFileRelPath: null,
    activeTab: "overview",
    demoEnabled: false,
    demoTour: null,
    demoCleanup: null,
    demoTourStepMs: DEMO_TOUR_STEP_MS_DEFAULT,
    demoAutoTourRequested: false,
    demoAutoTourTimerId: null,
  };

  const elements = {
    openPackBtn: document.getElementById("open-pack-btn"),
    exportSupportPackBtn: document.getElementById("export-support-pack-btn"),
    demoBadge: document.getElementById("demo-badge"),
    demoTourBtn: document.getElementById("demo-tour-btn"),
    dropZone: document.getElementById("drop-zone"),
    banner: document.getElementById("global-banner"),
    tabs: Array.from(document.querySelectorAll(".tab")),
    panels: {
      overview: document.getElementById("panel-overview"),
      claims: document.getElementById("panel-claims"),
      drift: document.getElementById("panel-drift"),
      decision: document.getElementById("panel-decision"),
      files: document.getElementById("panel-files"),
    },
    overviewPackPath: document.getElementById("overview-pack-path"),
    overviewPackSize: document.getElementById("overview-pack-size"),
    overviewPackSha: document.getElementById("overview-pack-sha"),
    verifyStatus: document.getElementById("verify-status"),
    verifyPath: document.getElementById("verify-path"),
    verifyMissing: document.getElementById("verify-missing"),
    verifySchemaErrors: document.getElementById("verify-schema-errors"),
    verifyHashMismatches: document.getElementById("verify-hash-mismatches"),
    verifyExtras: document.getElementById("verify-extras"),
    verifyCheckedCount: document.getElementById("verify-checked-count"),
    verifyTimestamp: document.getElementById("verify-timestamp"),
    verifyRaw: document.getElementById("verify-raw"),
    quickFilesCount: document.getElementById("quick-files-count"),
    quickClaimsCount: document.getElementById("quick-claims-count"),
    quickDriftCount: document.getElementById("quick-drift-count"),
    claimsTableBody: document.querySelector("#claims-table tbody"),
    claimDetailEmpty: document.getElementById("claim-detail-empty"),
    claimDetail: document.getElementById("claim-detail"),
    claimDetailTitle: document.getElementById("claim-detail-title"),
    claimDetailMeta: document.getElementById("claim-detail-meta"),
    claimEvidenceList: document.getElementById("claim-evidence-list"),
    claimNotes: document.getElementById("claim-notes"),
    claimAssumptions: document.getElementById("claim-assumptions"),
    driftSummary: document.getElementById("drift-summary"),
    driftChanges: document.getElementById("drift-changes"),
    driftMdEmpty: document.getElementById("drift-md-empty"),
    driftMdRender: document.getElementById("drift-md-render"),
    decisionEmpty: document.getElementById("decision-empty"),
    decisionFrame: document.getElementById("decision-frame"),
    decisionPdfActions: document.getElementById("decision-pdf-actions"),
    decisionOpenPdfBtn: document.getElementById("decision-open-pdf-btn"),
    decisionExportPdfBtn: document.getElementById("decision-export-pdf-btn"),
    decisionPrintPdfBtn: document.getElementById("decision-print-pdf-btn"),
    decisionPdfMessage: document.getElementById("decision-pdf-message"),
    filesTree: document.getElementById("files-tree"),
    previewEmpty: document.getElementById("preview-empty"),
    previewActions: document.getElementById("preview-actions"),
    previewOpenBtn: document.getElementById("preview-open-btn"),
    previewExportBtn: document.getElementById("preview-export-btn"),
    previewPrintBtn: document.getElementById("preview-print-btn"),
    previewNote: document.getElementById("preview-note"),
    previewWrap: document.getElementById("preview-wrap"),
    previewMeta: document.getElementById("preview-meta"),
    previewHtml: document.getElementById("preview-html"),
    previewText: document.getElementById("preview-text"),
  };

  function setBanner(message, isError) {
    if (!message) {
      elements.banner.classList.add("hidden");
      elements.banner.classList.remove("error");
      elements.banner.textContent = "";
      return;
    }
    elements.banner.textContent = message;
    elements.banner.classList.remove("hidden");
    elements.banner.classList.toggle("error", !!isError);
  }

  function isDemoEnabled(startupOptions) {
    const queryValue = new URLSearchParams(window.location.search).get("demo");
    const queryEnabled =
      queryValue === "1" || String(queryValue || "").trim().toLowerCase() === "true";
    const startupEnabled =
      startupOptions?.demoMode === true ||
      String(startupOptions?.demoMode || "")
        .trim()
        .toLowerCase() === "1";
    return queryEnabled || startupEnabled;
  }

  function getDemoQueryFlag(name) {
    const value = new URLSearchParams(window.location.search).get(name);
    return value === "1" || String(value || "").trim().toLowerCase() === "true";
  }

  function getDemoTourStepMsFromQuery() {
    const value = new URLSearchParams(window.location.search).get("tour_ms");
    if (value === null) {
      return DEMO_TOUR_STEP_MS_DEFAULT;
    }
    const parsed = Number.parseInt(value, 10);
    if (!Number.isFinite(parsed) || parsed < 300 || parsed > 10000) {
      return DEMO_TOUR_STEP_MS_DEFAULT;
    }
    return parsed;
  }

  function useDemoTour(tabs, setActiveTabFn, onStateChange, stepMs) {
    let timerId = null;
    let running = false;
    let stepIndex = 0;

    function clearTimer() {
      if (timerId !== null) {
        clearTimeout(timerId);
        timerId = null;
      }
    }

    function step() {
      if (!running) return;
      setActiveTabFn(tabs[stepIndex]);
      stepIndex = (stepIndex + 1) % tabs.length;
      timerId = setTimeout(step, stepMs);
    }

    function start() {
      if (running) return;
      running = true;
      stepIndex = 0;
      onStateChange();
      step();
    }

    function stop() {
      if (!running && timerId === null) return;
      running = false;
      stepIndex = 0;
      clearTimer();
      onStateChange();
    }

    function toggle() {
      if (running) {
        stop();
      } else {
        start();
      }
    }

    function dispose() {
      stop();
    }

    return {
      start,
      stop,
      toggle,
      dispose,
      isRunning: () => running,
    };
  }

  function updateDemoUi() {
    const enabled = state.demoEnabled;
    if (elements.demoBadge) {
      elements.demoBadge.classList.toggle("hidden", !enabled);
    }
    if (elements.demoTourBtn) {
      const running = state.demoTour?.isRunning() || false;
      elements.demoTourBtn.classList.toggle("hidden", !enabled);
      elements.demoTourBtn.textContent = running ? "Stop tour" : "Start tour";
      elements.demoTourBtn.setAttribute("aria-pressed", running ? "true" : "false");
    }
  }

  function isEditableTarget(target) {
    if (!(target instanceof Element)) return false;
    if (target.isContentEditable) return true;
    const tagName = target.tagName;
    return tagName === "INPUT" || tagName === "TEXTAREA" || tagName === "SELECT";
  }

  function activateDemoMode() {
    if (state.demoCleanup) return;

    state.demoTour = useDemoTour(
      DEMO_TOUR_SEQUENCE,
      setActiveTab,
      updateDemoUi,
      state.demoTourStepMs
    );

    const onTourButtonClick = () => {
      state.demoTour?.toggle();
      updateDemoUi();
    };

    const onKeyDown = (event) => {
      if (!state.demoEnabled) return;
      if (event.defaultPrevented) return;
      if (event.ctrlKey || event.altKey || event.metaKey) return;
      if (isEditableTarget(event.target)) return;

      const key = String(event.key || "").toLowerCase();
      if (DEMO_HOTKEY_TABS[key]) {
        state.demoTour?.stop();
        setActiveTab(DEMO_HOTKEY_TABS[key]);
        updateDemoUi();
        event.preventDefault();
        return;
      }

      if (key === " " || event.code === "Space") {
        state.demoTour?.toggle();
        updateDemoUi();
        event.preventDefault();
        return;
      }

      if (key === "escape") {
        state.demoTour?.stop();
        updateDemoUi();
        event.preventDefault();
      }
    };

    elements.demoTourBtn?.addEventListener("click", onTourButtonClick);
    document.addEventListener("keydown", onKeyDown);

    if (state.demoAutoTourRequested) {
      state.demoAutoTourTimerId = window.setTimeout(() => {
        if (!state.demoEnabled) return;
        state.demoTour?.start();
        updateDemoUi();
      }, DEMO_TOUR_AUTOSTART_DELAY_MS);
    }

    state.demoCleanup = () => {
      if (state.demoAutoTourTimerId !== null) {
        clearTimeout(state.demoAutoTourTimerId);
        state.demoAutoTourTimerId = null;
      }
      state.demoTour?.dispose();
      state.demoTour = null;
      elements.demoTourBtn?.removeEventListener("click", onTourButtonClick);
      document.removeEventListener("keydown", onKeyDown);
      state.demoCleanup = null;
    };
    window.addEventListener("beforeunload", state.demoCleanup, { once: true });
  }

  function setDemoEnabled(enabled) {
    if (state.demoEnabled === enabled) {
      updateDemoUi();
      return;
    }

    state.demoEnabled = enabled;
    if (enabled) {
      activateDemoMode();
    } else if (state.demoCleanup) {
      state.demoCleanup();
    }
    updateDemoUi();
  }

  function setActiveTab(tab) {
    state.activeTab = tab;
    elements.tabs.forEach((button) => {
      button.classList.toggle("active", button.dataset.tab === tab);
    });
    Object.keys(elements.panels).forEach((name) => {
      elements.panels[name].classList.toggle("active", name === tab);
    });
  }

  function humanSize(bytes) {
    if (typeof bytes !== "number") return "-";
    const units = ["B", "KB", "MB", "GB"];
    let value = bytes;
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit += 1;
    }
    return `${value.toFixed(unit === 0 ? 0 : 2)} ${units[unit]}`;
  }

  function escapeHtml(input) {
    return String(input)
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;");
  }

  function isGoodStatus(value) {
    return /\b(good|verified|ready)\b/.test(String(value || "").trim().toLowerCase());
  }

  function applyStatusTone(element, value) {
    if (!element) return;
    element.classList.toggle("status-good", isGoodStatus(value));
  }

  function renderOverview() {
    const pack = state.pack;
    if (!pack) {
      elements.overviewPackPath.textContent = "-";
      elements.overviewPackSize.textContent = "-";
      elements.overviewPackSha.textContent = "-";
      elements.verifyStatus.textContent = "-";
      elements.verifyStatus.classList.remove("status-good");
      elements.verifyPath.textContent = "-";
      elements.verifyRaw.textContent = "";
      elements.quickFilesCount.textContent = "0";
      elements.quickClaimsCount.textContent = "0";
      elements.quickDriftCount.textContent = "0";
      return;
    }

    const verify = pack.verification || {};
    const verifyStatus = verify.status || "-";
    elements.overviewPackPath.textContent = pack.packPath || "-";
    elements.overviewPackSize.textContent = humanSize(pack.packSizeBytes);
    elements.overviewPackSha.textContent = pack.packSha256 || "-";
    elements.verifyStatus.textContent = verifyStatus;
    applyStatusTone(elements.verifyStatus, verifyStatus);
    elements.verifyPath.textContent = verify.verifierPath || "not available";
    elements.verifyMissing.textContent = verify.missing ?? "-";
    elements.verifySchemaErrors.textContent = verify.schemaErrors ?? "-";
    elements.verifyHashMismatches.textContent = verify.hashMismatches ?? "-";
    elements.verifyExtras.textContent = verify.extras ?? "-";
    elements.verifyCheckedCount.textContent = verify.checkedEntriesCount ?? "-";
    elements.verifyTimestamp.textContent = verify.timestampUtc || "-";
    elements.verifyRaw.textContent = verify.raw
      ? JSON.stringify(verify.raw, null, 2)
      : verify.message || "";

    elements.quickFilesCount.textContent = String(Array.isArray(pack.files) ? pack.files.length : 0);
    elements.quickClaimsCount.textContent = String(
      pack.quickCounts?.claimsCount ?? (Array.isArray(pack.claims) ? pack.claims.length : 0)
    );
    elements.quickDriftCount.textContent = String(
      pack.quickCounts?.driftChangesCount ??
        (Array.isArray(pack.drift?.changes) ? pack.drift.changes.length : 0)
    );
  }

  function renderClaims() {
    const claims = state.pack?.claims || [];
    elements.claimsTableBody.innerHTML = "";
    state.selectedClaimId = claims.length > 0 ? claims[0].claimId : null;

    claims.forEach((claim) => {
      const tr = document.createElement("tr");
      tr.className = "claim-row";
      tr.dataset.claimId = claim.claimId;
      const claimStatus = claim.status || "";
      const statusClass = isGoodStatus(claimStatus) ? "status-good" : "";
      tr.innerHTML = `
        <td class="mono">${escapeHtml(claim.claimId || "")}</td>
        <td>${escapeHtml(claim.title || "")}</td>
        <td>${escapeHtml(claim.impact || "")}</td>
        <td class="${statusClass}">${escapeHtml(claimStatus)}</td>
        <td class="mono">${escapeHtml(claim.primaryEvidenceRelPath || "")}</td>
      `;
      tr.addEventListener("click", () => {
        state.selectedClaimId = claim.claimId;
        renderClaimDetail();
        highlightSelectedClaim();
      });
      elements.claimsTableBody.appendChild(tr);
    });

    highlightSelectedClaim();
    renderClaimDetail();
  }

  function highlightSelectedClaim() {
    elements.claimsTableBody.querySelectorAll("tr.claim-row").forEach((row) => {
      row.classList.toggle("selected", row.dataset.claimId === state.selectedClaimId);
    });
  }

  function renderClaimDetail() {
    const claims = state.pack?.claims || [];
    const claim = claims.find((item) => item.claimId === state.selectedClaimId);

    if (!claim) {
      elements.claimDetailEmpty.classList.remove("hidden");
      elements.claimDetail.classList.add("hidden");
      return;
    }

    elements.claimDetailEmpty.classList.add("hidden");
    elements.claimDetail.classList.remove("hidden");
    elements.claimDetailTitle.textContent = `${claim.claimId} - ${claim.title}`;
    elements.claimDetailMeta.textContent = `impact=${claim.impact || "-"} | status=${claim.status || "-"}`;

    elements.claimEvidenceList.innerHTML = "";
    (claim.evidence || []).forEach((evidence) => {
      const li = document.createElement("li");
      li.className = "mono";
      li.textContent = evidence.sha256
        ? `${evidence.relPath} (${evidence.sha256})`
        : evidence.relPath;
      elements.claimEvidenceList.appendChild(li);
    });

    elements.claimNotes.textContent = claim.notes || "-";
    elements.claimAssumptions.textContent = claim.assumptions || "-";
  }

  function renderDrift() {
    const drift = state.pack?.drift || { summary: {}, changes: [] };
    elements.driftSummary.textContent = JSON.stringify(drift.summary || {}, null, 2);

    elements.driftChanges.innerHTML = "";
    if (!Array.isArray(drift.changes) || drift.changes.length === 0) {
      const p = document.createElement("p");
      p.className = "small";
      p.textContent = "No drift changes in this pack.";
      elements.driftChanges.appendChild(p);
    } else {
      drift.changes.forEach((change) => {
        const card = document.createElement("article");
        card.className = "change";
        const affected = Array.isArray(change.affectedClaims) ? change.affectedClaims : [];
        const affectedHtml =
          affected.length === 0
            ? "<p class=\"small\">No affected claims listed.</p>"
            : `<ul>${affected
                .map(
                  (claim) =>
                    `<li><strong>${escapeHtml(claim.claimId || "-")}</strong> (${escapeHtml(
                      claim.impact || "-"
                    )})</li>`
                )
                .join("")}</ul>`;

        card.innerHTML = `
          <h3>${escapeHtml(change.kind || "unknown")} - <span class="mono">${escapeHtml(
          change.entryPath || "-"
        )}</span></h3>
          <details>
            <summary>Raw summary</summary>
            <pre class="json-box">${escapeHtml(
              JSON.stringify(change.summary || {}, null, 2)
            )}</pre>
          </details>
          <h4>Affected claims</h4>
          ${affectedHtml}
        `;
        elements.driftChanges.appendChild(card);
      });
    }

    if (drift.markdownHtml) {
      elements.driftMdEmpty.classList.add("hidden");
      elements.driftMdRender.innerHTML = drift.markdownHtml;
    } else {
      elements.driftMdEmpty.classList.remove("hidden");
      elements.driftMdRender.innerHTML = "";
    }
  }

  function renderDecisionPack() {
    const decision = state.pack?.decisionPack || {};
    const pdfRelPath = decision.pdfRelPath || null;
    elements.decisionPdfActions.classList.toggle("hidden", !pdfRelPath);
    elements.decisionPdfMessage.textContent = pdfRelPath
      ? `DecisionPack.pdf ready: ${pdfRelPath}`
      : "DecisionPack.pdf not present in this pack.";

    if (decision.html) {
      elements.decisionEmpty.classList.add("hidden");
      elements.decisionFrame.srcdoc = decision.html;
      elements.decisionFrame.classList.remove("hidden");
    } else {
      elements.decisionEmpty.classList.remove("hidden");
      elements.decisionFrame.srcdoc = "";
      elements.decisionFrame.classList.add("hidden");
    }
  }

  function createTree(files) {
    const root = {};
    files.forEach((entry) => {
      const parts = String(entry.relPath || "").split("/").filter(Boolean);
      let node = root;
      parts.forEach((part, idx) => {
        if (!node[part]) {
          node[part] = idx === parts.length - 1 ? null : {};
        }
        if (node[part] !== null) {
          node = node[part];
        }
      });
    });
    return root;
  }

  function sortedKeys(obj) {
    return Object.keys(obj).sort((a, b) => {
      const lowerCmp = a.toLowerCase().localeCompare(b.toLowerCase());
      if (lowerCmp !== 0) return lowerCmp;
      return a.localeCompare(b);
    });
  }

  function renderTreeNode(node, parentPath) {
    const ul = document.createElement("ul");
    sortedKeys(node).forEach((name) => {
      const li = document.createElement("li");
      const relPath = parentPath ? `${parentPath}/${name}` : name;
      const child = node[name];
      if (child === null) {
        const btn = document.createElement("button");
        btn.type = "button";
        btn.className = "file-btn mono";
        btn.textContent = name;
        btn.addEventListener("click", () => onFileClick(relPath));
        li.appendChild(btn);
      } else {
        const label = document.createElement("div");
        label.className = "small";
        label.textContent = name;
        li.appendChild(label);
        li.appendChild(renderTreeNode(child, relPath));
      }
      ul.appendChild(li);
    });
    return ul;
  }

  function renderFiles() {
    const files = state.pack?.files || [];
    elements.filesTree.innerHTML = "";
    if (files.length === 0) {
      elements.filesTree.textContent = "No extracted files available.";
    } else {
      const tree = createTree(files);
      elements.filesTree.appendChild(renderTreeNode(tree, ""));
    }
    resetPreview();
  }

  function fileExtension(relPath) {
    const normalized = String(relPath || "");
    const idx = normalized.lastIndexOf(".");
    if (idx < 0) return "";
    return normalized.slice(idx + 1).toLowerCase();
  }

  function setPreviewActions(relPath, allowPrintPdf) {
    state.selectedFileRelPath = relPath || null;
    elements.previewActions.classList.toggle("hidden", !relPath);
    elements.previewPrintBtn.classList.toggle("hidden", !allowPrintPdf);
  }

  function showActionOnly(relPath, note, allowPrintPdf) {
    elements.previewEmpty.classList.add("hidden");
    elements.previewWrap.classList.add("hidden");
    elements.previewMeta.textContent = "";
    elements.previewText.textContent = "";
    elements.previewHtml.classList.add("hidden");
    elements.previewHtml.innerHTML = "";
    setPreviewActions(relPath, !!allowPrintPdf);
    elements.previewNote.textContent = note || "";
    elements.previewNote.classList.toggle("hidden", !note);
  }

  function showPreview(preview) {
    const kind = String(preview.kind || "").toLowerCase();
    const relPath = preview.relPath || state.selectedFileRelPath;
    elements.previewEmpty.classList.add("hidden");
    elements.previewWrap.classList.remove("hidden");
    elements.previewMeta.textContent = `${relPath} | kind=${kind} | truncated=${
      preview.truncated ? "yes" : "no"
    }`;
    elements.previewText.textContent = preview.text || "";
    if (kind === "markdown" && preview.html) {
      elements.previewHtml.classList.remove("hidden");
      elements.previewHtml.innerHTML = preview.html;
    } else {
      elements.previewHtml.classList.add("hidden");
      elements.previewHtml.innerHTML = "";
    }
    elements.previewNote.textContent = "";
    elements.previewNote.classList.add("hidden");
    setPreviewActions(relPath, fileExtension(relPath) === "pdf");
  }

  function resetPreview() {
    state.selectedFileRelPath = null;
    elements.previewEmpty.classList.remove("hidden");
    elements.previewWrap.classList.add("hidden");
    elements.previewActions.classList.add("hidden");
    elements.previewNote.classList.add("hidden");
    elements.previewNote.textContent = "";
    elements.previewMeta.textContent = "";
    elements.previewText.textContent = "";
    elements.previewHtml.innerHTML = "";
    elements.previewHtml.classList.add("hidden");
  }

  async function onFileClick(relPath) {
    if (!invoke) return;

    const selectedRelPath = String(relPath || "");
    if (selectedRelPath.toLowerCase().endsWith("decisionpack.html")) {
      setActiveTab("decision");
      return;
    }

    try {
      const preview = await invoke("read_file_preview", { relPath: selectedRelPath });
      const kind = String(preview.kind || "").toLowerCase();
      if (kind === "json" || kind === "markdown" || kind === "text") {
        showPreview(preview);
      } else if (kind === "pdf") {
        showActionOnly(
          preview.relPath || selectedRelPath,
          "PDF preview is not available in-app. Use Open, Export, or Print.",
          true
        );
      } else {
        showActionOnly(
          preview.relPath || selectedRelPath,
          "Preview is not available for this file type. Use Open or Export.",
          false
        );
      }
      setActiveTab("files");
    } catch (error) {
      showActionOnly(
        selectedRelPath,
        `Preview unavailable for ${selectedRelPath}: ${String(error)}`,
        fileExtension(selectedRelPath) === "pdf"
      );
      setActiveTab("files");
    }
  }

  function renderAll() {
    renderOverview();
    renderClaims();
    renderDrift();
    renderDecisionPack();
    renderFiles();
  }

  async function loadPackFromPath(path) {
    if (!invoke || !path) return;
    setBanner("Loading pack...", false);
    try {
      const pack = await invoke("load_pack", { packPath: path });
      state.pack = pack;
      renderAll();

      const warnings = [];
      if (Array.isArray(pack.missingFiles) && pack.missingFiles.length > 0) {
        warnings.push(`Missing required files: ${pack.missingFiles.join(", ")}`);
      }
      if (Array.isArray(pack.parseWarnings) && pack.parseWarnings.length > 0) {
        warnings.push(`Parse warnings:\n- ${pack.parseWarnings.join("\n- ")}`);
      }
      setBanner(warnings.join("\n\n"), false);
    } catch (error) {
      state.pack = null;
      renderAll();
      setBanner(`Failed to load pack:\n${String(error)}`, true);
    }
  }

  async function openPackDialog() {
    if (!invoke) {
      setBanner(TAURI_FALLBACK_BANNER, true);
      return;
    }
    try {
      const path = await invoke("pick_pack_zip");
      if (path) {
        await loadPackFromPath(path);
      }
    } catch (error) {
      setBanner(`Open dialog failed:\n${String(error)}`, true);
    }
  }

  async function openPackFile(relPath) {
    if (!invoke || !relPath) return;
    await invoke("open_pack_file", { relPath });
  }

  async function exportPackFile(relPath) {
    if (!invoke || !relPath) return null;
    return invoke("export_pack_file", { relPath });
  }

  async function printPackPdf(relPath) {
    if (!invoke || !relPath) return null;
    return invoke("print_pack_pdf", { relPath });
  }

  async function onOpenSelectedFile() {
    if (!state.selectedFileRelPath) return;
    try {
      await openPackFile(state.selectedFileRelPath);
    } catch (error) {
      setBanner(`Failed to open file:\n${String(error)}`, true);
    }
  }

  async function onExportSelectedFile() {
    if (!state.selectedFileRelPath) return;
    try {
      const savedPath = await exportPackFile(state.selectedFileRelPath);
      if (savedPath) {
        setBanner(`Exported ${state.selectedFileRelPath} to:\n${savedPath}`, false);
      }
    } catch (error) {
      setBanner(`Failed to export file:\n${String(error)}`, true);
    }
  }

  async function onPrintSelectedPdf() {
    if (!state.selectedFileRelPath) return;
    try {
      const result = await printPackPdf(state.selectedFileRelPath);
      if (result?.message) {
        setBanner(result.message, false);
      }
    } catch (error) {
      setBanner(`Failed to print PDF:\n${String(error)}`, true);
    }
  }

  async function onDecisionOpenPdf() {
    const relPath = state.pack?.decisionPack?.pdfRelPath;
    if (!relPath) return;
    try {
      await openPackFile(relPath);
    } catch (error) {
      setBanner(`Failed to open DecisionPack.pdf:\n${String(error)}`, true);
    }
  }

  async function onDecisionExportPdf() {
    const relPath = state.pack?.decisionPack?.pdfRelPath;
    if (!relPath) return;
    try {
      const savedPath = await exportPackFile(relPath);
      if (savedPath) {
        setBanner(`Exported DecisionPack.pdf to:\n${savedPath}`, false);
      }
    } catch (error) {
      setBanner(`Failed to export DecisionPack.pdf:\n${String(error)}`, true);
    }
  }

  async function onDecisionPrintPdf() {
    const relPath = state.pack?.decisionPack?.pdfRelPath;
    if (!relPath) return;
    try {
      const result = await printPackPdf(relPath);
      if (result?.message) {
        setBanner(result.message, false);
      }
    } catch (error) {
      setBanner(`Failed to print DecisionPack.pdf:\n${String(error)}`, true);
    }
  }

  async function onExportSupportPack() {
    if (!invoke) {
      setBanner(TAURI_FALLBACK_BANNER, true);
      return;
    }
    try {
      const savedPath = await invoke("export_support_pack");
      if (savedPath) {
        setBanner(`Support pack exported:\n${savedPath}`, false);
      }
    } catch (error) {
      setBanner(`Failed to export support pack:\n${String(error)}`, true);
    }
  }

  function uriToPath(uri) {
    if (!uri || !uri.startsWith("file://")) return null;
    try {
      let decoded = decodeURIComponent(uri.replace("file://", ""));
      if (/^\/[A-Za-z]:\//.test(decoded)) {
        decoded = decoded.slice(1);
      }
      return decoded.replaceAll("/", "\\");
    } catch (_) {
      return null;
    }
  }

  function pathFromDropEvent(event) {
    const files = event.dataTransfer?.files;
    if (files && files.length > 0 && files[0].path) {
      return files[0].path;
    }
    const uriList = event.dataTransfer?.getData("text/uri-list");
    if (uriList) {
      const first = uriList.split(/\r?\n/).find((line) => line.trim().length > 0);
      const path = uriToPath(first);
      if (path) return path;
    }
    const plain = event.dataTransfer?.getData("text/plain");
    if (plain && plain.toLowerCase().endsWith(".zip")) {
      return plain.trim();
    }
    return null;
  }

  function attachDropHandlers() {
    const dropZone = elements.dropZone;
    const activate = () => dropZone.classList.add("drag-active");
    const deactivate = () => dropZone.classList.remove("drag-active");

    document.addEventListener("dragenter", activate);
    document.addEventListener("dragover", (event) => {
      event.preventDefault();
      activate();
    });
    document.addEventListener("dragleave", () => {
      deactivate();
    });
    document.addEventListener("drop", async (event) => {
      event.preventDefault();
      deactivate();
      const path = pathFromDropEvent(event);
      if (path) {
        await loadPackFromPath(path);
      }
    });
  }

  function attachTabHandlers() {
    elements.tabs.forEach((button) => {
      button.addEventListener("click", () => {
        setActiveTab(button.dataset.tab);
      });
    });
  }

  function bootstrapDropBridge() {
    window.__EPI_VIEWER_DROP__ = async function (paths) {
      if (!Array.isArray(paths) || paths.length === 0) {
        return;
      }
      await loadPackFromPath(paths[0]);
    };

    window.__EPI_VIEWER_SET_TAB__ = function (tabName) {
      if (elements.panels[tabName]) {
        setActiveTab(tabName);
      }
    };
  }

  async function applyStartupOptions() {
    if (!invoke) return null;
    try {
      const options = await invoke("get_startup_options");
      if (isDemoEnabled(options)) {
        setDemoEnabled(true);
      }
      if (options && options.autostartPack) {
        await loadPackFromPath(options.autostartPack);
      }
      if (options && options.autostartTab && elements.panels[options.autostartTab]) {
        setActiveTab(options.autostartTab);
      }
      return options;
    } catch (_) {
      // keep startup silent in viewer mode
      return null;
    }
  }

  async function init() {
    attachTabHandlers();
    attachDropHandlers();
    bootstrapDropBridge();

    state.demoTourStepMs = getDemoTourStepMsFromQuery();
    state.demoAutoTourRequested = getDemoQueryFlag("autotour");

    elements.openPackBtn.addEventListener("click", openPackDialog);
    elements.exportSupportPackBtn?.addEventListener("click", onExportSupportPack);
    elements.previewOpenBtn?.addEventListener("click", onOpenSelectedFile);
    elements.previewExportBtn?.addEventListener("click", onExportSelectedFile);
    elements.previewPrintBtn?.addEventListener("click", onPrintSelectedPdf);
    elements.decisionOpenPdfBtn?.addEventListener("click", onDecisionOpenPdf);
    elements.decisionExportPdfBtn?.addEventListener("click", onDecisionExportPdf);
    elements.decisionPrintPdfBtn?.addEventListener("click", onDecisionPrintPdf);
    setActiveTab("overview");
    renderAll();

    if (isDemoEnabled()) {
      setDemoEnabled(true);
    }

    if (!invoke) {
      setBanner(TAURI_FALLBACK_BANNER, true);
      return;
    }
    await applyStartupOptions();
  }

  init();
})();
