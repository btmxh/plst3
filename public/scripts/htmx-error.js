document.body.addEventListener("htmx:responseError", (e) => {
  const container = document.getElementById("diagnostics");
  if (container === null) {
    return;
  }

  const response = e.detail.xhr.response;
  const time = new Date().toString();
  const code = e.detail.xhr.status;
  container.insertAdjacentHTML("afterbegin", `<div>${time}: (${code}) ${response}</div>`);
});
