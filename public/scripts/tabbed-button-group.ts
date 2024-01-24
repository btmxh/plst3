for (const tabbedButtonGroup of document.getElementsByClassName(
  "tabbed-button-group"
)) {
  const buttons = [...tabbedButtonGroup.querySelectorAll(".tab-button")];
  const tabs = [...tabbedButtonGroup.querySelectorAll(".tab")];

  if (buttons.length !== tabs.length) {
    console.error("Mismatched tabbed-button-group");
    continue;
  }

  for (let i = 0; i < buttons.length; ++i) {
    buttons[i].addEventListener("click", () => {
      buttons.forEach((b) => b.classList.remove("active"));
      tabs.forEach((t) => t.classList.remove("active"));
      buttons[i].classList.add("active");
      tabs[i].classList.add("active");
    });
  }
}
