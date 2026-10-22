// Keep in sync with src/theme/theme.ts.
(function () {
  var preference = null;
  try {
    preference = localStorage.getItem("yaoplm.theme");
  } catch (_) {}
  var dark =
    preference === "dark" ||
    (preference !== "light" && window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", dark);
  document.documentElement.style.colorScheme = dark ? "dark" : "light";
})();
