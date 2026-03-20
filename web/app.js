var API_BASE = "/api";

/* ---- Auth Gate ---- */

(function checkAuth() {
  var token = sessionStorage.getItem("axiom_token");
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
      sessionStorage.removeItem("axiom_token");
      window.location.href = "login.html";
    }
  }).catch(function () {});
})();

function logout() {
  var token = sessionStorage.getItem("axiom_token");
  sessionStorage.removeItem("axiom_token");
  if (token) {
    fetch("/auth/logout", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ token: token })
    }).catch(function () {});
  }
  window.location.href = "login.html";
}

/* ---- Utilities ---- */

function fetchJSON(path) {
  return fetch(API_BASE + path).then(function (res) {
    if (!res.ok) {
      var err = new Error(res.status === 404 ? "Not found" : "Request failed (" + res.status + ")");
      err.status = res.status;
      throw err;
    }
    return res.json();
  });
}

function set(id, value) {
  var el = document.getElementById(id);
  if (!el) return;
  var sk = el.querySelector(".skeleton");
  if (sk) sk.remove();
  el.textContent = value;
}

function setHTML(id, html) {
  var el = document.getElementById(id);
  if (!el) return;
  el.innerHTML = html;
}

function show(id) {
  var el = document.getElementById(id);
  if (el) el.style.display = "";
}

function hide(id) {
  var el = document.getElementById(id);
  if (el) el.style.display = "none";
}

function formatNumber(n) {
  if (n == null) return "-";
  return String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");
}

function truncateHash(hash) {
  if (!hash || hash.length <= 16) return hash || "";
  return hash.slice(0, 8) + "\u2026" + hash.slice(-8);
}

function copyWithFeedback(text, el) {
  if (!text) return;
  if (navigator.clipboard) {
    navigator.clipboard.writeText(text).then(function () {
      if (el) {
        var orig = el.textContent;
        el.textContent = "copied";
        el.classList.add("copied");
        setTimeout(function () {
          el.textContent = orig;
          el.classList.remove("copied");
        }, 1200);
      }
    });
  }
}

function getQuery() {
  return new URLSearchParams(window.location.search);
}

function setHealth(id, ok) {
  var el = document.getElementById(id);
  if (!el) return;
  var dot = el.querySelector(".status-dot");
  if (dot) dot.className = "status-dot " + (ok ? "ok" : "err");
  var span = el.querySelector(".health-text");
  if (span) span.textContent = ok ? "ok" : "unavailable";
}

function statusBadge(active) {
  if (active) return '<span class="badge badge-success">active</span>';
  return '<span class="badge badge-danger">inactive</span>';
}

function timeAgo(isoString) {
  if (!isoString) return "";
  var diff = Math.max(0, Math.floor((Date.now() - new Date(isoString).getTime()) / 1000));
  if (diff < 5) return "just now";
  if (diff < 60) return diff + "s ago";
  if (diff < 3600) return Math.floor(diff / 60) + "m ago";
  if (diff < 86400) return Math.floor(diff / 3600) + "h ago";
  return Math.floor(diff / 86400) + "d ago";
}

/* ---- Toast Notifications ---- */

function showToast(title, message) {
  var container = document.getElementById("toastContainer");
  if (!container) return;
  var toast = document.createElement("div");
  toast.className = "toast";
  toast.innerHTML = '<div class="toast-title">' + title + '</div><div>' + message + '</div>';
  container.appendChild(toast);
  setTimeout(function () {
    toast.classList.add("toast-exit");
    setTimeout(function () {
      if (toast.parentNode) toast.parentNode.removeChild(toast);
    }, 300);
  }, 3500);
}

/* ---- Tooltip System ---- */

var tooltipEl = null;

function initTooltip() {
  tooltipEl = document.getElementById("tooltip");
  if (!tooltipEl) return;

  document.addEventListener("mouseover", function (e) {
    var target = e.target.closest("[data-tip]");
    if (target) {
      tooltipEl.textContent = target.getAttribute("data-tip");
      tooltipEl.classList.add("visible");
      positionTooltip(e);
    }
  });

  document.addEventListener("mousemove", function (e) {
    if (tooltipEl.classList.contains("visible")) {
      positionTooltip(e);
    }
  });

  document.addEventListener("mouseout", function (e) {
    var target = e.target.closest("[data-tip]");
    if (target) {
      tooltipEl.classList.remove("visible");
    }
  });
}

function positionTooltip(e) {
  if (!tooltipEl) return;
  var x = e.clientX + 12;
  var y = e.clientY + 12;
  if (x + 300 > window.innerWidth) x = e.clientX - 300;
  if (y + 40 > window.innerHeight) y = e.clientY - 40;
  tooltipEl.style.left = x + "px";
  tooltipEl.style.top = y + "px";
}

/* ---- Sparkline Charts ---- */

function renderSparkline(containerId, data, maxVal) {
  var container = document.getElementById(containerId);
  if (!container) return;
  container.innerHTML = "";
  if (!data || data.length === 0) return;
  var max = maxVal || Math.max.apply(null, data) || 1;
  data.forEach(function (val) {
    var bar = document.createElement("div");
    bar.className = "sparkline-bar";
    var pct = Math.min(100, Math.max(2, (val / max) * 100));
    bar.style.height = pct + "%";
    bar.setAttribute("data-tip", String(val));
    container.appendChild(bar);
  });
}

/* ---- Live Pulse ---- */

function setPulse(ok) {
  var dot = document.getElementById("livePulse");
  if (!dot) return;
  if (ok) {
    dot.classList.remove("inactive");
  } else {
    dot.classList.add("inactive");
  }
}

/* ---- Global Search ---- */

function initGlobalSearch() {
  var input = document.getElementById("globalSearch");
  var results = document.getElementById("searchResults");
  if (!input || !results) return;
  var debounce = null;

  input.addEventListener("input", function () {
    clearTimeout(debounce);
    var q = input.value.trim();
    if (!q) {
      results.classList.remove("open");
      return;
    }
    debounce = setTimeout(function () { doSearch(q, results); }, 250);
  });

  input.addEventListener("keydown", function (e) {
    if (e.key === "Escape") {
      results.classList.remove("open");
      input.blur();
    }
    if (e.key === "Enter") {
      var first = results.querySelector(".search-result-item");
      if (first) first.click();
    }
  });

  document.addEventListener("click", function (e) {
    if (!e.target.closest(".search-wrapper")) {
      results.classList.remove("open");
    }
  });
}

function doSearch(q, resultsEl) {
  resultsEl.innerHTML = "";
  var items = [];

  if (/^\d+$/.test(q)) {
    items.push({
      label: "Block",
      text: "Height " + q,
      href: "block.html?height=" + encodeURIComponent(q)
    });
  }

  if (/^(0x)?[a-fA-F0-9]{8,}$/.test(q)) {
    items.push({
      label: "Block",
      text: "Hash " + truncateHash(q),
      href: "block.html?hash=" + encodeURIComponent(q)
    });
    items.push({
      label: "Account",
      text: truncateHash(q),
      href: "accounts.html?id=" + encodeURIComponent(q)
    });
  }

  if (items.length === 0) {
    resultsEl.innerHTML = '<div class="search-no-results">No matching results</div>';
    resultsEl.classList.add("open");
    return;
  }

  items.forEach(function (item) {
    var div = document.createElement("div");
    div.className = "search-result-item";
    div.innerHTML = '<span class="sr-label">' + item.label + '</span>' + item.text;
    div.addEventListener("click", function () {
      window.location.href = item.href;
    });
    resultsEl.appendChild(div);
  });
  resultsEl.classList.add("open");
}

/* ---- Validators Renderer ---- */

function renderValidatorRows(tbody, validators) {
  if (!tbody) return;
  tbody.innerHTML = "";
  var list = validators || [];
  if (list.length === 0) {
    var tr = document.createElement("tr");
    tr.innerHTML = '<td colspan="6" class="empty">No validators</td>';
    tbody.appendChild(tr);
    return;
  }
  list.forEach(function (v) {
    var tr = document.createElement("tr");
    var idText = v.validator_id || "";
    var accountText = v.account_id || "";
    var stakeText = v.stake_amount != null ? formatNumber(v.stake_amount) : "-";
    var jailedText = v.jailed === true ? '<span class="badge badge-danger">jailed</span>' : '<span class="badge badge-muted">no</span>';
    if (v.jailed == null) jailedText = "-";
    tr.innerHTML =
      '<td data-tip="' + idText + '">' + truncateHash(idText) + "</td>" +
      "<td>" + (v.voting_power || "") + "</td>" +
      "<td>" + stakeText + "</td>" +
      '<td data-tip="' + accountText + '">' + truncateHash(accountText) + "</td>" +
      "<td>" + jailedText + "</td>" +
      "<td>" + statusBadge(v.active) + "</td>";
    tbody.appendChild(tr);
  });
}

/* ---- Overview ---- */

function initOverview() {
  var lastHeight = null;
  var txHistory = [];
  var blockTimeHistory = [];
  var firstLoad = true;

  fetchJSON("/blocks?limit=21").then(function (blocks) {
    if (blocks && blocks.length > 0) {
      blocks.reverse();
      blocks.forEach(function (b) {
        txHistory.push(b.transaction_count || 0);
      });
      if (txHistory.length > 20) txHistory = txHistory.slice(txHistory.length - 20);
      renderSparkline("sparkTx", txHistory, Math.max(5, Math.max.apply(null, txHistory)));

      for (var k = 1; k < blocks.length; k++) {
        var hDiff = blocks[k].height - blocks[k - 1].height;
        blockTimeHistory.push(hDiff > 0 ? 1 : 0);
      }
      if (blockTimeHistory.length > 20) blockTimeHistory = blockTimeHistory.slice(blockTimeHistory.length - 20);
      renderSparkline("sparkTime", blockTimeHistory, Math.max(3, Math.max.apply(null, blockTimeHistory)));
    }
  }).catch(function () {});

  function refresh() {
    fetchJSON("/status").then(function (s) {
      set("genesisHash", s.genesis_hash || "-");
      set("height", formatNumber(s.height));
      set("validatorCount", String(s.validator_count));
      set("syncing", s.syncing ? "Yes" : "No");
      set("protocolVersion", String(s.protocol_version != null ? s.protocol_version : "-"));

      set("latestHash", s.latest_block_hash || "-");
      var hashEl = document.getElementById("latestHash");
      if (hashEl) hashEl.setAttribute("data-tip", s.latest_block_hash || "");

      var isNewBlock = lastHeight !== null && s.height > lastHeight && !firstLoad;

      if (s.height > 0) {
        fetchJSON("/blocks/" + s.height).then(function (blk) {
          set("latestStateHash", blk.state_hash || "-");
          set("latestProposer", blk.proposer_id || "-");
          set("latestTxCount", String(blk.transaction_count || 0));

          var stateEl = document.getElementById("latestStateHash");
          if (stateEl) stateEl.setAttribute("data-tip", blk.state_hash || "");
          var propEl = document.getElementById("latestProposer");
          if (propEl) propEl.setAttribute("data-tip", blk.proposer_id || "");

          if (isNewBlock) {
            showToast("New Block", "Block #" + formatNumber(s.height) + " produced");
            txHistory.push(blk.transaction_count || 0);
            if (txHistory.length > 20) txHistory.shift();
          }

          renderSparkline("sparkTx", txHistory, 5);
        }).catch(function () {});
      } else {
        set("latestStateHash", "-");
        set("latestProposer", "-");
        set("latestTxCount", "0");
      }

      if (s.height !== lastHeight) {
        if (lastHeight !== null) {
          var produced = s.height - lastHeight;
          blockTimeHistory.push(produced);
          if (blockTimeHistory.length > 20) blockTimeHistory.shift();
        }
      }
      lastHeight = s.height;
      firstLoad = false;

      renderSparkline("sparkTime", blockTimeHistory, Math.max(5, Math.max.apply(null, blockTimeHistory) || 1));

      setPulse(true);

    }).catch(function () {
      setPulse(false);
    });

    fetchJSON("/consensus").then(function (c) {
      var txt = "h" + (c.next_height != null ? c.next_height : "-");
      if (c.lock && c.lock.height != null) {
        txt += " r" + c.lock.round;
        if (c.lock.block_hash) {
          txt += " lock " + truncateHash(c.lock.block_hash);
        } else {
          txt += " lock -";
        }
      }
      set("consensusInfo", txt);
    }).catch(function () {});
  }
  refresh();
  setInterval(refresh, 3000);

  fetch("/health/live").then(function (r) {
    setHealth("healthLive", r.ok);
  }).catch(function () { setHealth("healthLive", false); });

  fetch("/health/ready").then(function (r) {
    setHealth("healthReady", r.ok);
  }).catch(function () { setHealth("healthReady", false); });

  fetchJSON("/network/peers").then(function (peers) {
    var tbody = document.getElementById("peersTable");
    if (!tbody) return;
    tbody.innerHTML = "";
    var list = peers || [];
    if (list.length === 0) {
      var tr = document.createElement("tr");
      tr.innerHTML = '<td colspan="2" class="empty">No peers connected</td>';
      tbody.appendChild(tr);
      return;
    }
    list.forEach(function (p) {
      var tr = document.createElement("tr");
      tr.innerHTML =
        "<td>" + (p.address || "") + "</td>" +
        '<td class="time-relative">' + timeAgo(p.connected_since) + "</td>";
      tbody.appendChild(tr);
    });
  }).catch(function () {});

  fetchJSON("/validators").then(function (validators) {
    renderValidatorRows(document.getElementById("validatorsTable"), validators);
  }).catch(function () {});
}

/* ---- Blocks List ---- */

function initBlocks() {
  var errorEl = document.getElementById("blocksError");
  var tableEl = document.getElementById("blocksTable");
  var limitEl = document.getElementById("limitInput");
  var cursorEl = document.getElementById("cursorInput");
  var loadBtn = document.getElementById("loadBtn");

  function load() {
    errorEl.textContent = "";
    tableEl.innerHTML = "";
    var limit = limitEl.value || "50";
    var cursor = cursorEl.value || "";
    var qs = new URLSearchParams();
    qs.set("limit", limit);
    if (cursor) qs.set("cursor", cursor);

    fetchJSON("/blocks?" + qs.toString()).then(function (blocks) {
      if (!blocks || blocks.length === 0) {
        tableEl.innerHTML = '<tr><td colspan="6" class="empty">No blocks found</td></tr>';
        return;
      }
      blocks.forEach(function (b) {
        var tr = document.createElement("tr");
        tr.className = "clickable";
        var hashText = b.hash || "";
        var stateText = b.state_hash || "";
        var proposer = b.proposer_id || "";
        tr.innerHTML =
          "<td>" + formatNumber(b.height) + "</td>" +
          '<td class="truncate" data-tip="' + hashText + '">' + truncateHash(hashText) + "</td>" +
          '<td class="truncate" data-tip="' + proposer + '">' + truncateHash(proposer) + "</td>" +
          "<td>" + (b.transaction_count != null ? b.transaction_count : "0") + "</td>" +
          '<td class="time-relative">' + timeAgo(b.timestamp) + "</td>" +
          '<td class="truncate" data-tip="' + stateText + '">' + truncateHash(stateText) + "</td>";
        tr.addEventListener("click", function () {
          window.location.href = "block.html?height=" + encodeURIComponent(b.height);
        });
        tableEl.appendChild(tr);
      });
    }).catch(function (e) {
      errorEl.textContent = e.message;
    });
  }

  loadBtn.addEventListener("click", load);
  load();
}

/* ---- Block Detail ---- */

function initBlockDetail() {
  var errorEl = document.getElementById("blockError");
  var q = getQuery();
  var height = q.get("height");
  var hash = q.get("hash");
  var path = "";

  if (height != null && height !== "") {
    path = "/blocks/" + encodeURIComponent(height);
  } else if (hash) {
    path = "/blocks/by-hash/" + encodeURIComponent(hash);
  } else {
    errorEl.textContent = "No block specified.";
    return;
  }

  fetchJSON(path).then(function (b) {
    var h = b.height != null ? b.height : 0;

    set("blkHeight", formatNumber(h));
    set("blkEpoch", String(b.epoch != null ? b.epoch : ""));
    set("blkHash", b.hash || "");
    set("parentHash", b.parent_hash || "");
    set("blkProposer", b.proposer_id || "");
    set("stateHash", b.state_hash || "");
    set("blkTimestamp", b.timestamp ? timeAgo(b.timestamp) + " \u00b7 " + new Date(b.timestamp).toLocaleString() : "-");

    var blkHashEl = document.getElementById("blkHash");
    if (blkHashEl) blkHashEl.setAttribute("data-tip", b.hash || "");
    var parentEl = document.getElementById("parentHash");
    if (parentEl) parentEl.setAttribute("data-tip", b.parent_hash || "");
    var proposerEl = document.getElementById("blkProposer");
    if (proposerEl) proposerEl.setAttribute("data-tip", b.proposer_id || "");
    var stateEl = document.getElementById("stateHash");
    if (stateEl) stateEl.setAttribute("data-tip", b.state_hash || "");

    var copyLink = document.getElementById("copyHash");
    if (copyLink) {
      copyLink.addEventListener("click", function (e) {
        e.preventDefault();
        copyWithFeedback(b.hash || "", copyLink);
      });
    }

    var prevLink = document.getElementById("prevBlock");
    var nextLink = document.getElementById("nextBlock");
    if (prevLink) {
      if (h > 0) {
        prevLink.href = "block.html?height=" + (h - 1);
        prevLink.innerHTML = "&larr; Previous <span class='kbd-hint'>&larr;</span>";
      } else {
        prevLink.classList.add("disabled");
      }
    }
    if (nextLink) {
      nextLink.href = "block.html?height=" + (h + 1);
      nextLink.innerHTML = "Next &rarr; <span class='kbd-hint'>&rarr;</span>";
    }

    document.addEventListener("keydown", function (e) {
      if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA") return;
      if (e.key === "ArrowLeft" && prevLink && !prevLink.classList.contains("disabled")) {
        window.location.href = prevLink.href;
      }
      if (e.key === "ArrowRight" && nextLink) {
        window.location.href = nextLink.href;
      }
    });

    var txTable = document.getElementById("txTable");
    var txEmpty = document.getElementById("txEmpty");
    var txCount = document.getElementById("txCount");
    var txs = b.transactions || [];

    if (txCount) txCount.textContent = txs.length;

    if (txs.length === 0) {
      if (txEmpty) txEmpty.style.display = "block";
    } else {
      if (txEmpty) txEmpty.style.display = "none";
      txs.forEach(function (tx, i) {
        var tr = document.createElement("tr");
        var sender = tx.sender || "";
        var recipient = tx.recipient || "";
        tr.innerHTML =
          "<td>" + (i + 1) + "</td>" +
          '<td class="truncate" data-tip="' + sender + '">' + truncateHash(sender) + "</td>" +
          '<td class="truncate" data-tip="' + recipient + '">' + truncateHash(recipient) + "</td>" +
          "<td>" + formatNumber(tx.amount) + ' <span class="unit">AXM</span></td>' +
          "<td>" + (tx.nonce != null ? tx.nonce : "") + "</td>";
        txTable.appendChild(tr);
      });
    }

    var sigTable = document.getElementById("sigTable");
    var sigs = b.signatures || [];
    if (sigs.length === 0) {
      sigTable.innerHTML = '<tr><td colspan="2" class="empty">No signature data</td></tr>';
    } else {
      sigs.forEach(function (sig) {
        var tr = document.createElement("tr");
        var valText = sig.validator_id || "";
        var sigText = sig.signature || "";
        tr.innerHTML =
          '<td class="truncate" data-tip="' + valText + '">' + truncateHash(valText) + "</td>" +
          '<td class="truncate" data-tip="' + sigText + '">' + truncateHash(sigText) + "</td>";
        sigTable.appendChild(tr);
      });
    }
  }).catch(function (e) {
    errorEl.textContent = e.message;
  });
}

/* ---- Accounts ---- */

function initAccounts() {
  var input = document.getElementById("accountInput");
  var btn = document.getElementById("lookupBtn");
  var errorEl = document.getElementById("accountError");
  var resultSection = document.getElementById("accountResult");

  var q = getQuery();
  var prefill = q.get("id");
  if (prefill && input) {
    input.value = prefill;
    setTimeout(function () { lookup(); }, 100);
  }

  function lookup() {
    errorEl.textContent = "";
    if (resultSection) resultSection.style.display = "none";
    var id = (input.value || "").trim();
    if (!id) {
      errorEl.textContent = "Enter an account ID";
      return;
    }
    fetchJSON("/accounts/" + encodeURIComponent(id)).then(function (acc) {
      set("accId", acc.account_id || id);
      set("accBalance", formatNumber(acc.balance));
      set("accNonce", String(acc.nonce != null ? acc.nonce : ""));
      if (resultSection) resultSection.style.display = "block";
    }).catch(function (e) {
      errorEl.textContent = e.message;
    });
  }

  btn.addEventListener("click", lookup);
  input.addEventListener("keydown", function (e) {
    if (e.key === "Enter") lookup();
  });
}

/* ---- Validators ---- */

function initValidators() {
  var errorEl = document.getElementById("validatorsError");

  fetchJSON("/validators").then(function (validators) {
    var list = validators || [];
    var active = list.filter(function (v) { return v.active; });
    var hasStake = list.some(function (v) { return v.stake_amount != null; });
    var totalPower = list.reduce(function (sum, v) {
      return sum + (hasStake ? (v.stake_amount || 0) : (v.voting_power || 0));
    }, 0);
    var quorumThreshold = Math.floor(totalPower * 2 / 3);
    var quorumMin = quorumThreshold + 1;

    set("valCount", String(list.length));
    set("valActive", String(active.length));
    set("valTotalPower", String(totalPower));
    set("valQuorum", ">" + quorumThreshold + " (" + quorumMin + "+)");

    renderValidatorRows(document.getElementById("validatorsFullTable"), list);
  }).catch(function (e) {
    if (errorEl) errorEl.textContent = e.message;
  });
}

/* ---- Init ---- */

initTooltip();
initGlobalSearch();

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
}
