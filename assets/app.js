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
  document.addEventListener("change", function (event) {
    var target = event.target;
    if (!(target instanceof Element)) {
      return;
    }
    var picker = target.closest("[data-dir-target]");
    if (!picker) {
      return;
    }
    var value = picker.value;
    if (!value) {
      return;
    }
    var input = document.querySelector(picker.getAttribute("data-dir-target"));
    if (input) {
      input.value = value;
      input.focus();
    }
  });

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
