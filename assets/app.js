(function () {
  "use strict";

  document.addEventListener("submit", function (event) {
    var form = event.target;
    var message = form.getAttribute("data-confirm");
    if (message && !window.confirm(message)) {
      event.preventDefault();
    }
  });

  document.addEventListener("click", function (event) {
    var target = event.target;
    if (!(target instanceof Element)) {
      return;
    }
    var browse = target.closest("[data-browse-dir]");
    if (browse) {
      var input = document.querySelector(browse.getAttribute("data-dir-target"));
      if (input) {
        var value = input.value.trim();
        if (value) {
          browse.href = "/browse?path=" + encodeURIComponent(value);
        }
      }
      return;
    }
    var button = target.closest("[data-add-row]");
    if (!button) {
      return;
    }
    event.preventDefault();
    var template = document.getElementById("new-param-template");
    var container = document.getElementById("new-param-rows");
    if (template && container && template.content) {
      container.appendChild(template.content.cloneNode(true));
      var inputs = container.querySelectorAll(".newrow:last-child input");
      if (inputs.length > 0) {
        inputs[0].focus();
      }
    }
  });
  function initDirTree() {
    var tree = document.getElementById("dir-tree");
    if (!tree) {
      return;
    }
    var rootsUrl = tree.getAttribute("data-roots-url") || "/browse/children";
    var loaded = false;

    function load(container, path) {
      container.innerHTML = '<li class="tree-empty">加载中…</li>';
      var url = rootsUrl;
      if (path) {
        url += "?path=" + encodeURIComponent(path);
      }
      fetch(url, {
        credentials: "same-origin",
        headers: { Accept: "text/html" }
      })
        .then(function (response) {
          if (!response.ok) {
            throw new Error("HTTP " + response.status);
          }
          return response.text();
        })
        .then(function (html) {
          container.innerHTML = html;
        })
        .catch(function () {
          container.innerHTML =
            '<li class="tree-empty tree-error">加载失败，请重试</li>';
        });
    }

    document.addEventListener("click", function (event) {
      var target = event.target;
      if (!(target instanceof Element)) {
        return;
      }

      var opener = target.closest("[data-open-tree]");
      if (opener) {
        event.preventDefault();
        var card = document.getElementById("dir-tree-card");
        if (!card) {
          return;
        }
        card.hidden = false;
        if (!loaded) {
          loaded = true;
          load(tree, "");
        }
        card.scrollIntoView({ behavior: "smooth", block: "start" });
        return;
      }

      var toggle = target.closest(".tree-toggle");
      if (toggle) {
        event.preventDefault();
        var node = toggle.closest(".tree-node");
        var children = node ? node.querySelector(".tree-children") : null;
        if (!children) {
          return;
        }
        var expanded = toggle.getAttribute("aria-expanded") === "true";
        if (expanded) {
          children.hidden = true;
          toggle.setAttribute("aria-expanded", "false");
          toggle.textContent = "▶";
        } else {
          children.hidden = false;
          toggle.setAttribute("aria-expanded", "true");
          toggle.textContent = "▼";
          if (!children.dataset.loaded) {
            children.dataset.loaded = "1";
            load(children, node.getAttribute("data-path"));
          }
        }
        return;
      }

      var select = target.closest(".tree-select");
      if (select) {
        event.preventDefault();
        var treeNode = select.closest(".tree-node");
        var input = document.getElementById("share-path");
        if (treeNode && input) {
          input.value = treeNode.getAttribute("data-path") || "";
          input.focus();
        }
        var previous = document.querySelector(".tree-select.selected");
        if (previous) {
          previous.classList.remove("selected");
        }
        select.classList.add("selected");
      }
    });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", initDirTree);
  } else {
    initDirTree();
  }

  function localizeTimes() {
    var nodes = document.querySelectorAll("time[data-local-time]");
    var zone = "";
    try {
      zone = Intl.DateTimeFormat().resolvedOptions().timeZone || "";
    } catch (error) {
      zone = "";
    }
    for (var index = 0; index < nodes.length; index += 1) {
      var element = nodes[index];
      var iso = element.getAttribute("datetime");
      var date = new Date(iso);
      if (isNaN(date.getTime())) {
        continue;
      }
      element.textContent = date.toLocaleString(undefined, {
        year: "numeric",
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
        hour12: false
      });
      element.title = "UTC: " + iso + (zone ? " / " + zone : "");
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", localizeTimes);
  } else {
    localizeTimes();
  }
})();
