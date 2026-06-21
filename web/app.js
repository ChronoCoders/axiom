/* =========================================================
   AXIOM Console — app.js
   Vanilla JS. No frameworks. Auth via localStorage.
   ========================================================= */

var API_BASE = "/api";

/* ---- Auth ---- */

(function checkAuth() {
  var token = localStorage.getItem("axiom_token");
  if (!token) {
    window.location.href = "login.html";
    return;
  }
  fetch("/auth/verify", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ token: token })
  }).then(function (res) {
    if (!res.ok) {
      localStorage.removeItem("axiom_token");
      window.location.href = "login.html";
    }
  }).catch(function () {});
})();

function logout() {
  var token = localStorage.getItem("axiom_token");
  localStorage.removeItem("axiom_token");
  if (token) {
    fetch("/auth/logout", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ token: token })
    }).catch(function () {});
  }
  window.location.href = "login.html";
}

/* ---- API ---- */

function fetchJSON(path) {
  var token = localStorage.getItem("axiom_token");
  var headers = { "Content-Type": "application/json" };
  if (token) headers["Authorization"] = "Bearer " + token;
  return fetch(API_BASE + path, { headers: headers }).then(function (res) {
    if (!res.ok) {
      var err = new Error(res.status === 404 ? "Not found" : "Request failed (" + res.status + ")");
      err.status = res.status;
      throw err;
    }
    return res.json();
  });
}

/* ---- Shared status poll ---- */

var _statusCache = null;
var _statusCallbacks = [];

function subscribeStatus(fn) {
  _statusCallbacks.push(fn);
}

function _startStatusPoll() {
  function poll() {
    fetchJSON("/status").then(function (s) {
      _statusCache = s;
      _statusCallbacks.forEach(function (fn) { try { fn(s); } catch (_) {} });
    }).catch(function () {
      _statusCallbacks.forEach(function (fn) { try { fn(null); } catch (_) {} });
    });
  }
  poll();
  setInterval(poll, 1000);
}

/* ---- SSE block events ---- */

var _latestHashFromSSE = null;
var _sseCallbacks = [];

function subscribeBlock(fn) {
  _sseCallbacks.push(fn);
}

function _onBlockEvent(height, hash) {
  setHeight(height);
  _latestHashFromSSE = hash;
  _sseCallbacks.forEach(function (fn) { try { fn(height, hash); } catch (_) {} });
}

function _startSSE() {
  var es = new EventSource("/events");
  es.addEventListener("block", function (e) {
    try {
      var d = JSON.parse(e.data);
      _onBlockEvent(d.height, d.hash);
    } catch (_) {}
  });
}

/* ---- Height stepper ---- */

var _displayedHeight = null;
var _targetHeight    = null;
var _stepTimer       = null;

function _stepToTarget() {
  if (_displayedHeight === null || _targetHeight === null) return;
  if (_displayedHeight >= _targetHeight) {
    clearInterval(_stepTimer);
    _stepTimer = null;
    return;
  }
  _displayedHeight += 1;
  _renderHeight(_displayedHeight);
}

function _renderHeight(h) {
  setText("ovHeight",      fmt(h));
  setText("sidebarHeight", "h " + fmt(h));
}

function setHeight(newHeight) {
  if (_displayedHeight === null) {
    _displayedHeight = newHeight;
    _targetHeight    = newHeight;
    _renderHeight(newHeight);
    return;
  }
  if (newHeight <= _displayedHeight) return;
  _targetHeight = newHeight;
  var delta    = _targetHeight - _displayedHeight;
  var interval = Math.round(1000 / Math.max(1, delta));
  interval     = Math.max(100, Math.min(1000, interval));
  if (_stepTimer !== null) {
    clearInterval(_stepTimer);
    _stepTimer = null;
  }
  _stepTimer = setInterval(_stepToTarget, interval);
}

/* ---- DOM helpers ---- */

function el(id) { return document.getElementById(id); }

function setText(id, value) {
  var node = el(id);
  if (!node) return;
  var sk = node.querySelector(".skeleton");
  if (sk) sk.remove();
  node.textContent = value != null ? String(value) : "-";
}

function setHTML(id, html) {
  var node = el(id);
  if (node) node.innerHTML = html;
}

function show(id) { var node = el(id); if (node) node.style.display = ""; }
function hide(id) { var node = el(id); if (node) node.style.display = "none"; }

/* ---- Formatters ---- */

function fmt(n) {
  if (n == null) return "-";
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

function shortHash(hash) {
  if (!hash || hash.length <= 16) return hash || "";
  return hash.slice(0, 8) + "\u2026" + hash.slice(-8);
}

function timeAgo(ts) {
  if (!ts) return "";
  var t = typeof ts === "number" ? ts * 1000 : new Date(ts).getTime();
  var diff = Math.max(0, Math.floor((Date.now() - t) / 1000));
  if (diff < 5)     return "just now";
  if (diff < 60)    return diff + "s ago";
  if (diff < 3600)  return Math.floor(diff / 60) + "m ago";
  if (diff < 86400) return Math.floor(diff / 3600) + "h ago";
  return Math.floor(diff / 86400) + "d ago";
}

function fmtTimestamp(ts) {
  if (!ts) return "-";
  var t = typeof ts === "number" ? ts * 1000 : new Date(ts).getTime();
  return new Date(t).toUTCString().replace(" GMT", " UTC");
}

function statusBadge(active) {
  if (active) return '<span class="badge badge-green">Active</span>';
  return '<span class="badge badge-red">Inactive</span>';
}

function getQuery() { return new URLSearchParams(window.location.search); }

/* ---- Copy ---- */

function copyWithFeedback(text, btn) {
  if (!text || !navigator.clipboard) return;
  navigator.clipboard.writeText(text).then(function () {
    if (!btn) return;
    var orig = btn.innerHTML;
    btn.innerHTML = '<i class="fa-solid fa-check"></i> Copied';
    btn.classList.add("copied");
    setTimeout(function () {
      btn.innerHTML = orig;
      btn.classList.remove("copied");
    }, 1400);
  });
}

/* ---- Tooltip ---- */

var _tip = null;

function initTooltip() {
  _tip = el("tooltip");
  if (!_tip) return;
  document.addEventListener("mouseover", function (e) {
    var target = e.target.closest("[data-tip]");
    if (target) {
      _tip.textContent = target.getAttribute("data-tip");
      _tip.classList.add("visible");
    }
  });
  document.addEventListener("mousemove", function (e) {
    if (!_tip || !_tip.classList.contains("visible")) return;
    var x = e.clientX + 14, y = e.clientY + 14;
    if (x + 460 > window.innerWidth) x = e.clientX - 460;
    if (y + 50 > window.innerHeight) y = e.clientY - 50;
    _tip.style.left = x + "px";
    _tip.style.top  = y + "px";
  });
  document.addEventListener("mouseout", function (e) {
    if (e.target.closest("[data-tip]")) _tip.classList.remove("visible");
  });
}

/* ---- Sparkline ---- */

function renderSparkline(id, data) {
  var c = el(id);
  if (!c || !data || !data.length) return;
  c.innerHTML = "";
  var max = Math.max.apply(null, data) || 1;
  data.forEach(function (v) {
    var bar = document.createElement("div");
    bar.className = "sparkline-bar";
    bar.style.height = Math.max(4, Math.round((v / max) * 100)) + "%";
    bar.setAttribute("data-tip", String(v));
    c.appendChild(bar);
  });
}

/* ---- Pulse dot ---- */

function setPulse(ok) {
  var d = el("livePulse");
  if (!d) return;
  d.className = "pulse-dot" + (ok ? "" : " inactive");
  var s = el("chainStatus");
  if (s) s.textContent = ok ? "Chain live" : "Unreachable";
}

/* ---- Global search ---- */

function initSearch() {
  var input   = el("globalSearch");
  var results = el("searchResults");
  if (!input || !results) return;
  var debounce = null;

  input.addEventListener("input", function () {
    clearTimeout(debounce);
    var q = input.value.trim();
    if (!q) { results.classList.remove("open"); return; }
    debounce = setTimeout(function () { doSearch(q, results); }, 220);
  });

  input.addEventListener("keydown", function (e) {
    if (e.key === "Escape") { results.classList.remove("open"); input.blur(); }
    if (e.key === "Enter") {
      var first = results.querySelector(".search-result-item");
      if (first) first.click();
    }
  });

  document.addEventListener("click", function (e) {
    if (!e.target.closest(".search-wrapper")) results.classList.remove("open");
  });
}

function doSearch(q, resultsEl) {
  resultsEl.innerHTML = "";
  var items = [];

  if (/^\d+$/.test(q)) {
    items.push({ label: "Block", text: "Height " + q, href: "block.html?height=" + encodeURIComponent(q) });
  }

  if (/^(0x)?[a-fA-F0-9]{8,}$/.test(q)) {
    items.push({ label: "Block",   text: shortHash(q), href: "block.html?hash="    + encodeURIComponent(q) });
    items.push({ label: "Account", text: shortHash(q), href: "accounts.html?id=" + encodeURIComponent(q) });
  }

  if (items.length === 0) {
    resultsEl.innerHTML = '<div class="search-no-results">No matching results</div>';
    resultsEl.classList.add("open");
    return;
  }

  items.forEach(function (item) {
    var div = document.createElement("div");
    div.className = "search-result-item";
    div.innerHTML = '<span class="sr-label">' + item.label + "</span>" + item.text;
    div.addEventListener("click", function () { window.location.href = item.href; });
    resultsEl.appendChild(div);
  });
  resultsEl.classList.add("open");
}

/* ---- Sidebar status ticker ---- */

function initSidebarStatus() {
  subscribeStatus(function (s) {
    if (!s) { setPulse(false); return; }
    setHeight(s.height);
    setPulse(true);
  });
}

/* =========================================================
   PAGE: Overview (index.html)
   ========================================================= */

function initOverview() {
  var txHistory    = [];
  var blocksSeq    = 0;

  subscribeBlock(function (height, hash) {
    fetchJSON("/blocks/" + height).then(function (b) {
      var tbody = el("recentBlocksTable");
      if (!tbody) return;
      var tr = document.createElement("tr");
      tr.className = "clickable";
      tr.innerHTML =
        "<td>" + fmt(b.height) + "</td>" +
        '<td data-tip="' + (b.hash || "") + '">' + shortHash(b.hash) + "</td>" +
        '<td data-tip="' + (b.proposer_id || "") + '">' + shortHash(b.proposer_id) + "</td>" +
        "<td>" + (b.transaction_count || 0) + "</td>" +
        '<td class="text-muted">' + timeAgo(b.timestamp) + "</td>";
      tr.addEventListener("click", function () {
        window.location.href = "block.html?height=" + encodeURIComponent(b.height);
      });
      tbody.insertBefore(tr, tbody.firstChild);
      var rows = tbody.querySelectorAll("tr");
      if (rows.length > 10) tbody.removeChild(rows[rows.length - 1]);
    }).catch(function () {});
  });

  subscribeStatus(function (s) {
    if (!s) { setPulse(false); return; }
    setHeight(s.height);
    setText("ovValidators", fmt(s.validator_count));
    setText("ovSyncing",   s.syncing ? "Yes" : "No");
    var _proto = s.protocol_version;
    setText("ovProtocol", _proto === 1 ? "Transfer" : _proto === 2 ? "Staking" : _proto != null ? "v" + _proto : "-");

    var hashEl = el("ovLatestHash");
    if (hashEl) {
      hashEl.textContent = shortHash(s.latest_block_hash || "");
      hashEl.setAttribute("data-tip", s.latest_block_hash || "");
    }

    if (s.height > 0) {
      fetchJSON("/blocks/" + s.height).then(function (blk) {
        setText("ovStateHash",  shortHash(blk.state_hash || ""));
        setText("ovProposer",   shortHash(blk.proposer_id || ""));
        setText("ovTxCount",    fmt(blk.transaction_count || 0));
        setText("ovTimestamp",  timeAgo(blk.timestamp));

        var shEl = el("ovStateHash");
        if (shEl) shEl.setAttribute("data-tip", blk.state_hash || "");
      }).catch(function () {});
    }

    fetchJSON("/consensus").then(function (c) {
      var h = c.next_height != null ? fmt(c.next_height) : "-";
      var r = c.lock && c.lock.round != null ? c.lock.round : "-";
      setText("ovConsensus", "Next: " + h + " · Round " + r);
    }).catch(function () {});

    setPulse(true);
  });

  function refreshBlocks() {
    var seq = ++blocksSeq;
    fetchJSON("/blocks?limit=10").then(function (blocks) {
      if (seq !== blocksSeq) return;
      var tbody = el("recentBlocksTable");
      if (!tbody) return;
      if (!blocks || !blocks.length) {
        tbody.innerHTML = '<tr><td colspan="5" class="table-empty">No blocks yet</td></tr>';
        txHistory = [];
        renderSparkline("sparkTx", txHistory);
        return;
      }
      tbody.innerHTML = "";
      txHistory = [];
      blocks.forEach(function (b) {
        txHistory.push(b.transaction_count || 0);
        var tr = document.createElement("tr");
        tr.className = "clickable";
        tr.innerHTML =
          "<td>" + fmt(b.height) + "</td>" +
          '<td data-tip="' + (b.hash || "") + '">' + shortHash(b.hash) + "</td>" +
          '<td data-tip="' + (b.proposer_id || "") + '">' + shortHash(b.proposer_id) + "</td>" +
          "<td>" + (b.transaction_count || 0) + "</td>" +
          '<td class="text-muted">' + timeAgo(b.timestamp) + "</td>";
        tr.addEventListener("click", function () {
          window.location.href = "block.html?height=" + encodeURIComponent(b.height);
        });
        tbody.appendChild(tr);
      });
      renderSparkline("sparkTx", txHistory);
    }).catch(function () {});
  }

  function refreshPeers() {
    fetchJSON("/network/peers").then(function (peers) {
      var tbody = el("peersTable");
      if (!tbody) return;
      var list = peers || [];
      if (!list.length) {
        tbody.innerHTML = '<tr><td colspan="2" class="table-empty">No peers connected</td></tr>';
        return;
      }
      tbody.innerHTML = "";
      list.forEach(function (p) {
        var tr = document.createElement("tr");
        tr.innerHTML =
          "<td>" + (p.address || "") + "</td>" +
          '<td class="text-muted">' + timeAgo(p.connected_since) + "</td>";
        tbody.appendChild(tr);
      });
    }).catch(function () {});
  }

  function refreshHealth() {
    fetch("/health/live").then(function (r) { setHealthDot("healthLiveDot", r.ok); setHealthLabel("healthLiveLabel", r.ok ? "Live" : "Down"); }).catch(function () { setHealthDot("healthLiveDot", false); setHealthLabel("healthLiveLabel", "Down"); });
    fetch("/health/ready").then(function (r) { setHealthDot("healthReadyDot", r.ok); setHealthLabel("healthReadyLabel", r.ok ? "Ready" : "Not ready"); }).catch(function () { setHealthDot("healthReadyDot", false); setHealthLabel("healthReadyLabel", "Not ready"); });
  }

  function setHealthDot(id, ok) {
    var d = el(id);
    if (d) d.className = "health-dot " + (ok ? "ok" : "err");
  }

  function setHealthLabel(id, text) {
    var lbl = el(id);
    if (lbl) lbl.textContent = text;
  }

  refreshBlocks();
  refreshPeers();
  refreshHealth();

  setInterval(refreshPeers, 15000);
}

/* =========================================================
   PAGE: Blocks list (blocks.html)
   ========================================================= */

function initBlocks() {
  var limitEl   = el("limitInput");
  var tableEl   = el("blocksTable");
  var errorEl   = el("blocksError");
  var rangeEl   = el("blocksRange");
  var newerBtn  = el("newerBtn");
  var olderBtn  = el("olderBtn");
  var latestBtn = el("latestBtn");

  var currentMin = null;
  var currentMax = null;

  function load(cursor) {
    if (errorEl) errorEl.textContent = "";
    if (tableEl) tableEl.innerHTML = '<tr><td colspan="6" class="table-empty">Loading...</td></tr>';
    var limit = (limitEl && limitEl.value) ? parseInt(limitEl.value, 10) : 50;
    var qs = new URLSearchParams();
    qs.set("limit", limit);
    if (cursor != null) qs.set("cursor", cursor);

    fetchJSON("/blocks?" + qs.toString()).then(function (blocks) {
      if (!blocks || !blocks.length) {
        tableEl.innerHTML = '<tr><td colspan="6" class="table-empty">No blocks found</td></tr>';
        currentMin = null;
        currentMax = null;
        updateControls(limit, cursor);
        return;
      }

      currentMax = blocks[0].height;
      currentMin = blocks[blocks.length - 1].height;

      tableEl.innerHTML = "";
      blocks.forEach(function (b) {
        var tr = document.createElement("tr");
        tr.className = "clickable";
        tr.innerHTML =
          "<td>" + fmt(b.height) + "</td>" +
          '<td data-tip="' + (b.hash || "") + '">' + shortHash(b.hash) + "</td>" +
          '<td data-tip="' + (b.proposer_id || "") + '">' + shortHash(b.proposer_id) + "</td>" +
          "<td>" + (b.transaction_count != null ? b.transaction_count : "0") + "</td>" +
          '<td class="text-muted">' + timeAgo(b.timestamp) + "</td>" +
          '<td data-tip="' + (b.state_hash || "") + '">' + shortHash(b.state_hash) + "</td>";
        tr.addEventListener("click", function () {
          window.location.href = "block.html?height=" + encodeURIComponent(b.height);
        });
        tableEl.appendChild(tr);
      });

      updateControls(limit, cursor);
    }).catch(function (e) {
      if (errorEl) errorEl.textContent = e.message;
      if (tableEl) tableEl.innerHTML = "";
    });
  }

  function updateControls(limit, cursor) {
    if (rangeEl) {
      if (currentMin != null && currentMax != null) {
        rangeEl.textContent = "Heights " + fmt(currentMin) + " – " + fmt(currentMax);
      } else {
        rangeEl.textContent = "";
      }
    }
    var atLatest = cursor == null;
    if (newerBtn) newerBtn.disabled = atLatest || currentMax == null;
    if (olderBtn) olderBtn.disabled = currentMin == null || currentMin <= 1;
  }

  if (newerBtn) newerBtn.addEventListener("click", function () {
    if (currentMax == null) return;
    var limit = (limitEl && limitEl.value) ? parseInt(limitEl.value, 10) : 50;
    var newCursor = currentMax + limit + 1;
    load(newCursor);
  });

  if (olderBtn) olderBtn.addEventListener("click", function () {
    if (currentMin == null) return;
    load(currentMin);
  });

  if (latestBtn) latestBtn.addEventListener("click", function () {
    load(null);
  });

  if (limitEl) limitEl.addEventListener("change", function () {
    load(null);
  });

  load(null);
}

/* =========================================================
   PAGE: Block detail (block.html)
   ========================================================= */

function initBlockDetail() {
  var q      = getQuery();
  var height = q.get("height");
  var hash   = q.get("hash");
  var path   = "";
  var errorEl = el("blockError");

  if (height != null && height !== "") {
    path = "/blocks/" + encodeURIComponent(height);
  } else if (hash) {
    path = "/blocks/by-hash/" + encodeURIComponent(hash);
  } else {
    if (errorEl) errorEl.textContent = "No block specified.";
    return;
  }

  fetchJSON(path).then(function (b) {
    var h = b.height != null ? b.height : 0;

    setText("blkHeight",    fmt(h));
    setText("blkEpoch",     b.epoch != null ? String(b.epoch) : "0");
    setText("blkTimestamp", fmtTimestamp(b.timestamp));
    setText("blkTxCount",   fmt(b.transaction_count || 0));

    var fields = {
      blkHash:    b.hash,
      parentHash: b.parent_hash,
      blkProposer:b.proposer_id,
      stateHash:  b.state_hash
    };

    Object.keys(fields).forEach(function (id) {
      var node = el(id);
      if (!node) return;
      node.textContent = shortHash(fields[id] || "");
      node.setAttribute("data-tip", fields[id] || "");
    });

    var copyBtn = el("copyHash");
    if (copyBtn) {
      copyBtn.addEventListener("click", function (e) {
        e.preventDefault();
        copyWithFeedback(b.hash || "", copyBtn);
      });
    }

    var prevLink = el("prevBlock");
    var nextLink = el("nextBlock");
    if (prevLink) {
      if (h > 0) { prevLink.href = "block.html?height=" + (h - 1); }
      else        { prevLink.classList.add("disabled"); }
    }
    if (nextLink) {
      nextLink.href = "block.html?height=" + (h + 1);
    }

    document.addEventListener("keydown", function (e) {
      if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
      if (e.key === "ArrowLeft"  && prevLink && !prevLink.classList.contains("disabled")) window.location.href = prevLink.href;
      if (e.key === "ArrowRight" && nextLink) window.location.href = nextLink.href;
    });

    var txTable  = el("txTable");
    var txEmpty  = el("txEmpty");
    var txs = b.transactions || [];

    if (!txs.length) {
      if (txEmpty) txEmpty.style.display = "block";
    } else {
      if (txEmpty) txEmpty.style.display = "none";
      txs.forEach(function (tx, i) {
        var tr = document.createElement("tr");
        tr.innerHTML =
          "<td>" + (i + 1) + "</td>" +
          '<td data-tip="' + (tx.sender || "") + '">' + shortHash(tx.sender) + "</td>" +
          '<td data-tip="' + (tx.recipient || "") + '">' + shortHash(tx.recipient) + "</td>" +
          "<td>" + fmt(tx.amount) + ' <span class="unit">AXM</span></td>' +
          "<td>" + (tx.nonce != null ? tx.nonce : "") + "</td>";
        txTable.appendChild(tr);
      });
    }

    var sigTable = el("sigTable");
    var sigs = b.signatures || [];
    if (!sigs.length) {
      sigTable.innerHTML = '<tr><td colspan="2" class="table-empty">No signatures</td></tr>';
    } else {
      sigs.forEach(function (sig) {
        var tr = document.createElement("tr");
        tr.innerHTML =
          '<td data-tip="' + (sig.validator_id || "") + '">' + shortHash(sig.validator_id) + "</td>" +
          '<td data-tip="' + (sig.signature || "") + '">' + shortHash(sig.signature) + "</td>";
        sigTable.appendChild(tr);
      });
    }
  }).catch(function (e) {
    if (errorEl) errorEl.textContent = e.message;
  });
}

/* =========================================================
   PAGE: Accounts (accounts.html)
   ========================================================= */

function initAccounts() {
  var input     = el("accountInput");
  var btn       = el("lookupBtn");
  var errorEl   = el("accountError");
  var resultEl  = el("accountResult");

  var prefill = getQuery().get("id");
  if (prefill && input) {
    input.value = prefill;
    setTimeout(lookup, 80);
  }

  function lookup() {
    if (errorEl) errorEl.textContent = "";
    if (resultEl) resultEl.style.display = "none";
    var id = (input ? input.value : "").trim();
    if (!id) { if (errorEl) errorEl.textContent = "Enter an account ID."; return; }

    fetchJSON("/accounts/" + encodeURIComponent(id)).then(function (acc) {
      var idNode = el("accId");
      if (idNode) {
        idNode.textContent = acc.account_id || id;
        idNode.setAttribute("data-tip", acc.account_id || id);
      }
      setText("accBalance", fmt(acc.balance));
      setText("accNonce",   acc.nonce != null ? String(acc.nonce) : "0");
      if (resultEl) resultEl.style.display = "";
    }).catch(function (e) {
      if (errorEl) errorEl.textContent = e.message;
    });
  }

  if (btn)   btn.addEventListener("click", lookup);
  if (input) input.addEventListener("keydown", function (e) { if (e.key === "Enter") lookup(); });
}

/* =========================================================
   PAGE: Validators (validators.html)
   ========================================================= */

function initValidators() {
  var errorEl = el("validatorsError");

  fetchJSON("/validators").then(function (validators) {
    var list    = validators || [];
    var active  = list.filter(function (v) { return v.active; });
    var hasSt   = list.some(function (v) { return v.stake_amount != null; });
    var total   = list.reduce(function (s, v) {
      return s + (hasSt ? (v.stake_amount || 0) : (v.voting_power || 0));
    }, 0);
    var quorumMin = Math.floor(total * 2 / 3) + 1;

    setText("valCount",   fmt(list.length));
    setText("valActive",  fmt(active.length));
    setText("valTotal",   fmt(total));
    setText("valQuorum",  fmt(quorumMin) + "+ required");

    var tbody = el("validatorsTable");
    if (!tbody) return;
    tbody.innerHTML = "";

    if (!list.length) {
      tbody.innerHTML = '<tr><td colspan="6" class="table-empty">No validators</td></tr>';
      return;
    }

    list.forEach(function (v) {
      var tr  = document.createElement("tr");
      var vid = v.validator_id || "";
      var aid = v.account_id  || "";
      var jailed = v.jailed === true
        ? '<span class="badge badge-red">Jailed</span>'
        : '<span class="badge badge-muted">No</span>';

      tr.innerHTML =
        '<td data-tip="' + vid + '">' + shortHash(vid) + "</td>" +
        "<td>" + fmt(v.voting_power) + "</td>" +
        "<td>" + (v.stake_amount != null ? fmt(v.stake_amount) : "-") + "</td>" +
        '<td data-tip="' + aid + '">' + shortHash(aid) + "</td>" +
        "<td>" + jailed + "</td>" +
        "<td>" + statusBadge(v.active) + "</td>";
      tbody.appendChild(tr);
    });
  }).catch(function (e) {
    if (errorEl) errorEl.textContent = e.message;
  });
}

/* =========================================================
   PAGE: Staking (staking.html)
   ========================================================= */

function initStaking() {
  var errEl = el("stakingError");

  fetchJSON("/staking").then(function (s) {
    var enabled = s.enabled === true;
    setText("stEnabled",   enabled ? "Active" : "Inactive (V1)");
    setText("stEpoch",     fmt(s.epoch));
    setText("stMinStake",  fmt(s.minimum_stake) + " AXM");
    setText("stUnbonding", fmt(s.unbonding_period) + " blocks");
    setText("stEvidence",  fmt(s.processed_evidence_count));

    var notice = el("stakingNotice");
    if (notice) notice.style.display = enabled ? "none" : "";

    var stakesEl = el("stakesTable");
    if (stakesEl) {
      var list = s.stakes || [];
      if (!list.length) {
        stakesEl.innerHTML = '<tr><td colspan="2" class="table-empty">No active stakes</td></tr>';
      } else {
        stakesEl.innerHTML = "";
        list.forEach(function (st) {
          var tr = document.createElement("tr");
          tr.innerHTML = '<td data-tip="' + (st.validator_id || "") + '">' + shortHash(st.validator_id) + "</td>" +
                         "<td>" + fmt(st.amount) + " AXM</td>";
          stakesEl.appendChild(tr);
        });
      }
    }

    var unbondEl = el("unbondingTable");
    if (unbondEl) {
      var queue = s.unbonding_queue || [];
      if (!queue.length) {
        unbondEl.innerHTML = '<tr><td colspan="3" class="table-empty">No unbonding entries</td></tr>';
      } else {
        unbondEl.innerHTML = "";
        queue.forEach(function (u) {
          var tr = document.createElement("tr");
          tr.innerHTML = '<td data-tip="' + (u.validator_id || "") + '">' + shortHash(u.validator_id) + "</td>" +
                         "<td>" + fmt(u.amount) + " AXM</td>" +
                         "<td>" + fmt(u.release_height) + "</td>";
          unbondEl.appendChild(tr);
        });
      }
    }

    var jailedEl = el("jailedTable");
    if (jailedEl) {
      var jailed = s.jailed_validators || [];
      if (!jailed.length) {
        jailedEl.innerHTML = '<tr><td class="table-empty">No jailed validators</td></tr>';
      } else {
        jailedEl.innerHTML = "";
        jailed.forEach(function (vid) {
          var tr = document.createElement("tr");
          tr.innerHTML = '<td data-tip="' + vid + '">' + shortHash(vid) + ' <span class="badge badge-red">Jailed</span></td>';
          jailedEl.appendChild(tr);
        });
      }
    }
  }).catch(function (e) {
    if (errEl) errEl.textContent = e.message;
  });
}

/* =========================================================
   PAGE: Transactions (tx.html)
   ========================================================= */

function hexToBytes(hex) {
  var out = new Uint8Array(hex.length / 2);
  for (var i = 0; i < hex.length; i += 2) out[i >> 1] = parseInt(hex.slice(i, i + 2), 16);
  return out;
}

function bytesToHex(bytes) {
  var out = "";
  for (var i = 0; i < bytes.length; i++) out += ("0" + bytes[i].toString(16)).slice(-2);
  return out;
}

function _writeU32BE(n, buf) {
  buf.push((n >>> 24) & 0xff, (n >>> 16) & 0xff, (n >>> 8) & 0xff, n & 0xff);
}

function _writeU64BE(n, buf) {
  var lo = n % 0x100000000;
  var hi = Math.floor(n / 0x100000000);
  _writeU32BE(hi >>> 0, buf);
  _writeU32BE(lo >>> 0, buf);
}

function _writeStr(s, buf) {
  _writeU32BE(s.length, buf);
  for (var i = 0; i < s.length; i++) buf.push(s.charCodeAt(i));
}

function serializeTxV1(sender, recipient, amount, nonce) {
  var buf = [];
  _writeStr(sender, buf);
  _writeStr(recipient, buf);
  _writeU64BE(amount, buf);
  _writeU64BE(nonce, buf);
  return new Uint8Array(buf);
}

function serializeTxV2(txType, sender, recipient, amount, nonce) {
  var buf = [];
  buf.push(txType & 0xff);
  _writeStr(sender, buf);
  _writeStr(recipient, buf);
  _writeU64BE(amount, buf);
  _writeU64BE(nonce, buf);
  return new Uint8Array(buf);
}

function initTransactions() {
  var privKeyEl = el("txPrivKey");
  var senderEl  = el("txSender");
  var typeEl    = el("txType");
  var recipEl   = el("txRecipient");
  var recipHint = el("txRecipientHint");
  var amountEl  = el("txAmount");
  var nonceEl   = el("txNonce");
  var submitBtn = el("txSubmit");
  var errEl     = el("txError");
  var resultEl  = el("txResult");
  var txHashOut = el("txHashOut");

  var txTypeHints = {
    Transfer: "Recipient account to receive the funds.",
    Stake:    "Validator account ID to stake with.",
    Unstake:  "Validator account ID to unstake from."
  };

  if (typeEl) typeEl.addEventListener("change", function () {
    if (recipHint) recipHint.textContent = txTypeHints[typeEl.value] || "";
  });

  function deriveAndFillSender() {
    var hex = privKeyEl ? privKeyEl.value.trim().replace(/^0x/i, "") : "";
    if (hex.length !== 64 || !/^[0-9a-fA-F]+$/.test(hex)) {
      if (senderEl) senderEl.value = "";
      return;
    }
    try {
      var kp = nacl.sign.keyPair.fromSeed(hexToBytes(hex));
      var pub = bytesToHex(kp.publicKey);
      if (senderEl) senderEl.value = pub;
      fetchJSON("/accounts/" + pub).then(function (acc) {
        if (nonceEl && acc.nonce != null) nonceEl.value = acc.nonce;
      }).catch(function () {});
    } catch (_) {
      if (senderEl) senderEl.value = "";
    }
  }

  if (privKeyEl) privKeyEl.addEventListener("input", deriveAndFillSender);

  if (submitBtn) submitBtn.addEventListener("click", function () {
    if (errEl)    errEl.textContent = "";
    if (resultEl) resultEl.style.display = "none";

    var privHex   = privKeyEl ? privKeyEl.value.trim().replace(/^0x/i, "") : "";
    var sender    = senderEl  ? senderEl.value.trim() : "";
    var txTypeTxt = typeEl    ? typeEl.value : "Transfer";
    var recipient = recipEl   ? recipEl.value.trim().replace(/^0x/i, "") : "";
    var amount    = parseInt(amountEl ? amountEl.value : "0", 10) || 0;
    var nonce     = parseInt(nonceEl  ? nonceEl.value  : "0", 10) || 0;

    var txTypeNum = { Transfer: 0, Stake: 1, Unstake: 2 }[txTypeTxt];
    if (txTypeNum === undefined) txTypeNum = 0;

    if (!privHex || privHex.length !== 64) {
      if (errEl) errEl.textContent = "Enter a valid 32-byte (64 hex char) private key.";
      return;
    }
    if (!recipient || !/^[0-9a-fA-F]{64}$/.test(recipient)) {
      if (errEl) errEl.textContent = "Recipient must be a 64-char hex account ID.";
      return;
    }
    if (amount <= 0) {
      if (errEl) errEl.textContent = "Amount must be greater than 0.";
      return;
    }

    submitBtn.disabled    = true;
    submitBtn.textContent = "Signing…";

    fetchJSON("/status").then(function (s) {
      var height = s.height || 0;
      var v2     = height >= 10000;

      var kp  = nacl.sign.keyPair.fromSeed(hexToBytes(privHex));
      var msg = v2 ? serializeTxV2(txTypeNum, sender, recipient, amount, nonce)
                   : serializeTxV1(sender, recipient, amount, nonce);
      var sig = nacl.sign.detached(msg, kp.secretKey);

      var body = {
        sender:    sender,
        recipient: recipient,
        amount:    amount,
        nonce:     nonce,
        signature: bytesToHex(sig),
        tx_type:   txTypeTxt
      };

      var token   = localStorage.getItem("axiom_token");
      var headers = { "Content-Type": "application/json" };
      if (token) headers["Authorization"] = "Bearer " + token;

      return fetch(API_BASE + "/transactions", {
        method:  "POST",
        headers: headers,
        body:    JSON.stringify(body)
      }).then(function (res) {
        return res.json().then(function (data) { return { ok: res.ok, data: data }; });
      });
    }).then(function (r) {
      submitBtn.disabled    = false;
      submitBtn.textContent = "Sign & Submit";
      if (r.ok) {
        if (txHashOut) txHashOut.textContent = r.data.tx_hash || "";
        if (resultEl)  resultEl.style.display = "";
        if (nonceEl)   nonceEl.value = (parseInt(nonceEl.value || "0", 10) + 1);
      } else {
        if (errEl) errEl.textContent = r.data.error || "Submission failed.";
      }
    }).catch(function (e) {
      submitBtn.disabled    = false;
      submitBtn.textContent = "Sign & Submit";
      if (errEl) errEl.textContent = e.message || "Network error.";
    });
  });

  var copyHashBtn = el("copyTxHash");
  if (copyHashBtn) copyHashBtn.addEventListener("click", function () {
    copyWithFeedback(txHashOut ? txHashOut.textContent : "", copyHashBtn);
  });
}

/* =========================================================
   Bootstrap
   ========================================================= */

initTooltip();
initSearch();
initSidebarStatus();
_startStatusPoll();
_startSSE();

var loc = window.location.pathname;
if (loc.endsWith("/") || loc.endsWith("index.html")) {
  initOverview();
} else if (loc.endsWith("blocks.html")) {
  initBlocks();
} else if (loc.endsWith("block.html")) {
  initBlockDetail();
} else if (loc.endsWith("accounts.html")) {
  initAccounts();
} else if (loc.endsWith("validators.html")) {
  initValidators();
} else if (loc.endsWith("staking.html")) {
  initStaking();
} else if (loc.endsWith("tx.html")) {
  initTransactions();
}
