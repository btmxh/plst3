function createYoutubePlayer(
  playerId: string,
  onStateChange: (e: YT.OnStateChangeEvent) => void,
  onError: (e: YT.OnErrorEvent) => void,
): Promise<YT.Player> {
  let ytPlayer: YT.Player | undefined = undefined;
  let resolveFn: (p: YT.Player) => void = () => {};
  (window as any).onYouTubeIframeAPIReady = () => {
    const tempPlayer = new YT.Player(playerId, {
      width: "100%",
      height: "100%",
      playerVars: {
        playsinline: 1,
        autoplay: 1,
        enablejsapi: 1,
        modestbranding: 0,
        cc_lang_pref: "en",
      },
      events: {
        onReady: () => {
          ytPlayer = tempPlayer;
          resolveFn(ytPlayer);
          console.log("yt player loaded");
        },
        onStateChange,
        onError,
      },
    });
  };

  const tag = document.createElement("script");
  tag.src = "https://www.youtube.com/iframe_api";
  const firstScriptTag = document.getElementsByTagName("script")[0];
  firstScriptTag!.parentNode!.insertBefore(tag, firstScriptTag);

  return new Promise((resolve) => {
    resolveFn = resolve;
    if (ytPlayer !== undefined) {
      resolve(ytPlayer);
      console.log("yt player loaded");
    }
  });
}

const cache = new Map<string, Promise<YT.Player>>();

export function getCachedYoutubePlayer(
  playerId: string,
  onStateChange: (e: YT.OnStateChangeEvent) => void = () => {},
  onError: (e: YT.OnErrorEvent) => void = () => {},
  cacheId = ""
): Promise<YT.Player> {
  const cachedPlayer = cache.get(cacheId);
  if (cachedPlayer !== undefined) {
    return cachedPlayer;
  }

  const player = createYoutubePlayer(playerId, onStateChange, onError);
  cache.set(cacheId, player);
  return player;
}

export async function stopAllPlayers() {
  for (const player of cache.values()) {
    const syncPlayer = await Promise.race([player, Promise.resolve(undefined)]);
    if (syncPlayer !== undefined) {
      syncPlayer.stopVideo();
    }
  }
}
