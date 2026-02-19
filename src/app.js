(function () {
  const invoke =
    (window.__TAURI__ &&
      ((window.__TAURI__.core && window.__TAURI__.core.invoke) ||
        (window.__TAURI__.tauri && window.__TAURI__.tauri.invoke))) ||
    window.__TAURI_INVOKE__ ||
    (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);

  const state = {
    pack: null,
    selectedClaimId: null,
    activeTab: "overview",
  };

  const elements = {
    openPackBtn: document.getElementById("open-pack-btn"),
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
    quickClaimsCount: document.getElementById("quick-claims-count"),
    quickDriftCount: document.getElementById("quick-drift-count"),
    quickAffectedClaimsCount: document.getElementById("quick-affected-claims-count"),
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
    filesTree: document.getElementById("files-tree"),
    previewEmpty: document.getElementById("preview-empty"),
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

  function renderOverview() {
    const pack = state.pack;
    if (!pack) {
      elements.overviewPackPath.textContent = "-";
      elements.overviewPackSize.textContent = "-";
      elements.overviewPackSha.textContent = "-";
      elements.verifyStatus.textContent = "-";
      elements.verifyPath.textContent = "-";
      elements.verifyRaw.textContent = "";
      elements.quickClaimsCount.textContent = "0";
      elements.quickDriftCount.textContent = "0";
      elements.quickAffectedClaimsCount.textContent = "0";
      return;
    }

    const verify = pack.verification || {};
    elements.overviewPackPath.textContent = pack.packPath || "-";
    elements.overviewPackSize.textContent = humanSize(pack.packSizeBytes);
    elements.overviewPackSha.textContent = pack.packSha256 || "-";
    elements.verifyStatus.textContent = verify.status || "-";
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

    elements.quickClaimsCount.textContent = String(pack.quickCounts?.claimsCount ?? 0);
    elements.quickDriftCount.textContent = String(pack.quickCounts?.driftChangesCount ?? 0);
    elements.quickAffectedClaimsCount.textContent = String(
      pack.quickCounts?.affectedClaimsCount ?? 0
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
      tr.innerHTML = `
        <td class="mono">${escapeHtml(claim.claimId || "")}</td>
        <td>${escapeHtml(claim.title || "")}</td>
        <td>${escapeHtml(claim.impact || "")}</td>
        <td>${escapeHtml(claim.status || "")}</td>
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
    const tree = createTree(files);
    elements.filesTree.appendChild(renderTreeNode(tree, ""));
    resetPreview();
  }

  function resetPreview() {
    elements.previewEmpty.classList.remove("hidden");
    elements.previewWrap.classList.add("hidden");
    elements.previewMeta.textContent = "";
    elements.previewText.textContent = "";
    elements.previewHtml.innerHTML = "";
    elements.previewHtml.classList.add("hidden");
  }

  async function onFileClick(relPath) {
    if (!invoke) return;

    if (relPath.toLowerCase().endsWith("decisionpack.html")) {
      setActiveTab("decision");
      return;
    }

    try {
      const preview = await invoke("read_file_preview", { relPath });
      elements.previewEmpty.classList.add("hidden");
      elements.previewWrap.classList.remove("hidden");
      elements.previewMeta.textContent = `${preview.relPath} | kind=${preview.kind} | truncated=${
        preview.truncated ? "yes" : "no"
      }`;

      if (preview.kind === "markdown" && preview.html) {
        elements.previewHtml.classList.remove("hidden");
        elements.previewHtml.innerHTML = preview.html;
      } else {
        elements.previewHtml.classList.add("hidden");
        elements.previewHtml.innerHTML = "";
      }

      elements.previewText.textContent = preview.text || "";
      setActiveTab("files");
    } catch (error) {
      setBanner(`Failed to preview file:\n${String(error)}`, true);
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
      setBanner("Tauri API unavailable. Start this app through Tauri.", true);
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
    if (!invoke) {
      return;
    }
    try {
      const options = await invoke("get_startup_options");
      if (options && options.autostartPack) {
        await loadPackFromPath(options.autostartPack);
      }
      if (options && options.autostartTab && elements.panels[options.autostartTab]) {
        setActiveTab(options.autostartTab);
      }
    } catch (_) {
      // keep startup silent in viewer mode
    }
  }

  async function init() {
    attachTabHandlers();
    attachDropHandlers();
    bootstrapDropBridge();

    elements.openPackBtn.addEventListener("click", openPackDialog);
    setActiveTab("overview");
    renderAll();
    if (!invoke) {
      setBanner("Tauri API unavailable. Start this app through Tauri.", true);
      return;
    }
    await applyStartupOptions();
  }

  init();
})();
