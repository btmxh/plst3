const header = document.getElementsByClassName("header")[0];

const threshold = 500;
let lastPress: number | undefined = undefined;
window.addEventListener("keydown", (ev) => {
  if (ev.key !== "Escape") {
    return;
  }

  const now = Date.now();
  if (lastPress !== undefined && now - lastPress < threshold) {
    header.classList.toggle("hidden");
    ev.preventDefault();
    lastPress = undefined;
  } else {
    lastPress = now;
  }
});

const tooltips = document.getElementsByClassName("toggle-header-tooltip");
for(const tooltip of tooltips) {
  tooltip.addEventListener("click", (ev) => {
    header.classList.toggle("hidden");
  })
}
