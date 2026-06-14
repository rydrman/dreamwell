const OFFLINE_HTML = `<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <meta name="theme-color" content="#000000" />
    <title>Dreamwell</title>
    <style>
      body {
        margin: 0;
        font-family: system-ui, sans-serif;
        background: #000;
        color: #fff;
        display: grid;
        place-items: center;
        min-height: 100vh;
        padding: 1.5rem;
        text-align: center;
      }
      p { max-width: 20rem; line-height: 1.5; }
    </style>
  </head>
  <body>
    <p>Dreamwell needs a network connection. Reconnect and reload the app.</p>
  </body>
</html>`;

self.addEventListener("install", (event) => {
  event.waitUntil(self.skipWaiting());
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") {
    return;
  }

  event.respondWith(
    fetch(event.request)
      .then((response) => {
        const url = new URL(event.request.url);
        if (
          url.pathname.startsWith("/api/") &&
          (response.status === 401 || response.status === 403)
        ) {
          self.clients.matchAll({ type: "window" }).then((clients) => {
            clients.forEach((client) => client.navigate(client.url));
          });
        }
        return response;
      })
      .catch(() => {
      if (event.request.mode === "navigate") {
        return new Response(OFFLINE_HTML, {
          headers: { "Content-Type": "text/html; charset=utf-8" },
        });
      }
      return Response.error();
    }),
  );
});
