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
})();
