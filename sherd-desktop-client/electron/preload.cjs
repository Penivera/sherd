const { contextBridge, ipcRenderer } = require("electron");

// Minimal surface: the renderer can ask main to run the OAuth loopback
// dance for a given provider, and gets back a one-time exchange code.
// Nothing else from Node/Electron is exposed.
contextBridge.exposeInMainWorld("sherd", {
  oauthLogin: (provider, apiBaseUrl) => ipcRenderer.invoke("oauth-login", provider, apiBaseUrl),
  vmRequest: (request) => ipcRenderer.invoke("vm-request", request),
});
