const { app, BrowserWindow, ipcMain, shell } = require("electron");
const path = require("node:path");
const http = require("node:http");

const isDev = process.env.NODE_ENV === "development";

function createWindow() {
  const win = new BrowserWindow({
    width: 480,
    height: 800,
    minWidth: 380,
    minHeight: 600,
    title: "Sherd",
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  if (isDev) {
    win.loadURL("http://localhost:5173");
    win.webContents.openDevTools({ mode: "detach" });
  } else {
    win.loadFile(path.join(__dirname, "..", "dist", "index.html"));
  }
}

// --- OAuth desktop loopback handoff ---
// sherd-auth's /auth/{provider}/login accepts a desktop_redirect_uri and,
// once the browser-based OAuth flow finishes, redirects the SYSTEM BROWSER
// (not this window) to that loopback URL with a one-time ?code=. We spin up
// a short-lived local HTTP server to catch that redirect, then hand the
// code back to the renderer to exchange via POST /auth/token/exchange.
//
// This intentionally never touches nodeIntegration/remote APIs from the
// renderer — it's all driven from here in the main process over IPC.
function waitForLoopbackCode(port) {
  return new Promise((resolve, reject) => {
    const server = http.createServer((req, res) => {
      const url = new URL(req.url, `http://127.0.0.1:${port}`);
      if (url.pathname !== "/callback") {
        res.writeHead(404).end();
        return;
      }
      const code = url.searchParams.get("code");
      res.writeHead(200, { "Content-Type": "text/html" });
      res.end(
        code
          ? "<html><body style='font-family:sans-serif'>Signed in. You can close this tab and return to Sherd.</body></html>"
          : "<html><body style='font-family:sans-serif'>Sign-in failed. You can close this tab and return to Sherd.</body></html>"
      );
      server.close();
      if (code) resolve(code);
      else reject(new Error("OAuth callback did not include a code"));
    });

    server.listen(port, "127.0.0.1");

    // Don't hang forever if the user closes the browser tab without finishing.
    const timeout = setTimeout(() => {
      server.close();
      reject(new Error("Timed out waiting for OAuth sign-in"));
    }, 5 * 60 * 1000);

    server.on("close", () => clearTimeout(timeout));
    server.on("error", (err) => {
      clearTimeout(timeout);
      reject(err);
    });
  });
}

function getEphemeralPort() {
  return new Promise((resolve, reject) => {
    const probe = http.createServer();
    probe.listen(0, "127.0.0.1", () => {
      const { port } = probe.address();
      probe.close(() => resolve(port));
    });
    probe.on("error", reject);
  });
}

ipcMain.handle("oauth-login", async (_event, provider, apiBaseUrl) => {
  if (!["google", "github"].includes(provider)) {
    throw new Error(`Unsupported OAuth provider: ${provider}`);
  }

  const port = await getEphemeralPort();
  const redirectUri = `http://127.0.0.1:${port}/callback`;
  const codePromise = waitForLoopbackCode(port);

  const loginUrl = `${apiBaseUrl}/auth/${provider}/login?desktop_redirect_uri=${encodeURIComponent(redirectUri)}`;
  await shell.openExternal(loginUrl);

  const code = await codePromise;
  return { code };
});

app.whenReady().then(() => {
  createWindow();

  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
