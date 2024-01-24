export class ReconnectableSocket {
  socket: WebSocket | undefined = undefined;
  retryCount = 0;
  onmessage: (msg: string) => void;
  messageQueue: string[] = [];

  constructor(onmessage: (msg: string) => void) {
    this.onmessage = onmessage;
    // `socket` will be properly initialized in `#init()`
    this.socket = null!;
    this.#init();
  }

  #init() {
    const pid = document.querySelector("main")?.dataset.pid;
    const scheme = location.protocol === "https:" ? "wss:" : "ws:";
    const wssUri = `${scheme}//${location.host}/watch/${pid}/ws`;
    console.log(`Attempting to connect to WebSocket endpoint at ${wssUri}`);
    this.socket = new WebSocket(wssUri);
    this.socket.onopen = () => {
      console.log("WebSocket connection established");
      if (this.socket !== undefined) {
        for (const msg of this.messageQueue) {
          console.log("message sented:", msg);
          this.socket.send(msg);
        }

        this.messageQueue = [];
      }
    };

    this.socket.onerror = (ev) => {
      console.error("WebSocket error: ", ev);
    };

    this.socket.onmessage = (msg) => {
      this.onmessage(msg.data as string);
    };

    this.socket.onclose = (ev) => {
      this.socket = undefined;
      console.error("WebSocket closed: ", ev);
      // Abnormal Closure/Service Restart/Try Again Later
      if ([1006, 1012, 1013].includes(ev.code)) {
        const exp = Math.min(this.retryCount, 6);
        const maxDelay = 1000 * Math.pow(2, exp);
        const delay = maxDelay * Math.random();
        console.log(`Retrying in ${delay}ms`);
        setTimeout(() => this.#init(), delay);
      }
    };
  }

  send(msg: string) {
    if (this.socket !== undefined) {
      console.log("message sented:", msg);
      this.socket.send(msg);
    } else {
      this.messageQueue.push(msg);
    }
  }
}
