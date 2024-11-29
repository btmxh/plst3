import { ReconnectableSocket } from "./watch-socket.js";
import {
  getCachedYoutubePlayer,
  stopAllPlayers as stopAllYTPlayers,
} from "./yt-embed-player.js";

let socket: ReconnectableSocket | undefined = undefined;

const serverVideoPlayer = document.getElementById(
  "server-player",
) as HTMLVideoElement;
serverVideoPlayer.addEventListener("ended", (e) => socket?.send("next"));
serverVideoPlayer.addEventListener("pause", (e) => socket?.send("pause"));
serverVideoPlayer.addEventListener("play", (e) => socket?.send("play"));

const stopAllPlayers = async () => {
  await stopAllYTPlayers();
  serverVideoPlayer.pause();
};

let current: any;

const pid = document.querySelector("main")!.dataset.pid!;

function getYoutubeId(url: string): string {
  const prefix = "https://youtu.be/";
  return url.substring(prefix.length);
}

const fetchPlayer = async () => {
  current = await fetch(`/playlist/${pid}/api/current`).then((r) => r.json());
  const playerWrapper = document.getElementById("player-wrapper")!;
  for (const child of playerWrapper.children) {
    child.classList.remove("active");
  }

  await stopAllPlayers();

  if (current?.media_type === "yt") {
    const ytPlayer = await getCachedYoutubePlayer("yt-player", (e) => {
      if (e.data === YT.PlayerState.ENDED) {
        socket?.send("next");
      } else if (e.data === YT.PlayerState.PAUSED) {
        socket?.send("pause");
      } else if (e.data === YT.PlayerState.PLAYING) {
        socket?.send("play");
      }
    }, () => {
      console.debug(socket);
      socket?.send("next");
    });
    const ytPlayerWrapper = document.getElementById("yt-player-wrapper")!;
    ytPlayerWrapper.style.aspectRatio = current.aspectRatio ?? "16/9";
    ytPlayerWrapper.classList.add("active");
    const id = getYoutubeId(current.url);
    console.debug(id);
    ytPlayer.loadVideoById(id);
    ytPlayer.playVideo();
    return;
  }

  if (current?.media_type === "local") {
    document.getElementById("server-player-wrapper")?.classList.add("active");
    serverVideoPlayer.currentTime = 0;
    serverVideoPlayer.setAttribute("src", `/servermedia/${current.id}`)
    serverVideoPlayer.load();
    try {
      await serverVideoPlayer.play();
    } catch (e) {
      console.error("autoplay not permitted", e);
    }
    return;
  }
};

const playerPlay = async () => {
  if (current?.media_type === "yt") {
    const player = await getCachedYoutubePlayer("yt-player");
    player.playVideo();
  } else if (current?.media_type === "local") {
    serverVideoPlayer.play();
  }
};

const playerPause = async () => {
  if (current?.media_type === "yt") {
    const player = await getCachedYoutubePlayer("yt-player");
    player.pauseVideo();
  } else if (current?.media_type === "local") {
    serverVideoPlayer.pause();
  }
};

const playerPlaying = async () => {
  if (current?.media_type === "yt") {
    const player = await getCachedYoutubePlayer("yt-player");
    return player.getPlayerState() !== YT.PlayerState.PAUSED;
  } else if (current?.media_type === "local") {
    return !serverVideoPlayer.paused;
  }

  return false;
};

socket = new ReconnectableSocket(async (msg) => {
  document.body.dispatchEvent(new Event(msg));
  if (msg === "media-changed") {
    fetchPlayer();
  } else if (msg === "play") {
    playerPlay();
  } else if (msg === "pause") {
    playerPause();
  } else if (msg === "playpause") {
    if (await playerPlaying()) {
      playerPause();
    } else {
      playerPlay();
    }
  }
});

fetchPlayer();
