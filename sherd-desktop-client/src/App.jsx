import React, { useState, useEffect } from "react";
import { Zap, Network, Terminal, Check, Circle, Sun, Moon, LogOut } from "lucide-react";
import AuthScreen from "./components/AuthScreen.jsx";
import { getCurrentSessionUser, getAccessToken } from "./services/session.js";
import { logout as logoutLocally } from "./services/auth.js";
import { connectToMesh } from "./services/meshService.js";
import { listNodes } from "./services/nodeService.js";
import { simulateTask as runSimulatedTask } from "./services/taskService.js";

const ORANGE = "#F16852";
const LIGHT_ORANGE = "#FEEADF";

export default function App() {
  // splash -> auth -> toggle -> mesh
  const [screen, setScreen] = useState("splash");
  const [isDark, setIsDark] = useState(false);
  const [user, setUser] = useState(null);
  const [authMethod, setAuthMethod] = useState(null); // { walletAddress, kind } when signed in via Solana

  useEffect(() => {
    if (screen === "splash") {
      const t = setTimeout(() => {
        // If a valid, non-expired session already exists (e.g. relaunching
        // the app shortly after signing in), skip straight past auth.
        const existing = getAccessToken() ? getCurrentSessionUser() : null;
        if (existing) {
          setUser(existing);
          setScreen("toggle");
        } else {
          setScreen("auth");
        }
      }, 2000);
      return () => clearTimeout(t);
    }
  }, [screen]);

  // NOTE: the original UI reset to the splash screen on every
  // visibilitychange, which breaks the Google/GitHub OAuth flow (the
  // desktop app loses focus while the system browser is up, then regains
  // it on the loopback redirect — that used to wipe all app state back to
  // splash mid-login). Removed for the desktop auth flow to work.

  function handleAuthenticated(authedUser, extra) {
    setUser(authedUser);
    setAuthMethod(extra?.walletAddress ? extra : null);
    setScreen("toggle");
  }

  function handleLogout() {
    logoutLocally();
    setUser(null);
    setAuthMethod(null);
    setScreen("auth");
  }

  return (
    <div className={`min-h-screen font-sans ${isDark ? "bg-neutral-950" : "bg-white"}`}>
      <ThemeToggle isDark={isDark} onToggle={() => setIsDark((d) => !d)} />
      {screen === "splash" && <Splash isDark={isDark} />}
      {screen === "auth" && <AuthScreen isDark={isDark} onAuthenticated={handleAuthenticated} />}
      {screen === "toggle" && <TogglePage isDark={isDark} onEnabled={() => setScreen("mesh")} />}
      {screen === "mesh" && (
        <MeshClient isDark={isDark} user={user} authMethod={authMethod} onLogout={handleLogout} />
      )}
    </div>
  );
}

// ---- Theme toggle button, fixed top-right on every screen ----

function ThemeToggle({ isDark, onToggle }) {
  return (
    <button
      onClick={onToggle}
      className="fixed top-4 right-4 z-20 h-10 w-10 rounded-full flex items-center justify-center transition-colors"
      style={{
        backgroundColor: isDark ? "#262626" : "#f5f5f5",
        color: isDark ? "#f5f5f5" : "#525252",
      }}
      aria-label="Toggle theme"
    >
      {isDark ? <Sun size={18} /> : <Moon size={18} />}
    </button>
  );
}

// ---- Reusable glow background ----
// Soft radial gradient blur behind the hero content, matching the reference.
function GlowBackground({ isDark }) {
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 overflow-hidden"
      style={{ zIndex: 0 }}
    >
      <div
        style={{
          position: "absolute",
          top: 0,
          left: "50%",
          transform: "translateX(-50%)",
          width: "120%",
          maxWidth: "900px",
          height: "700px",
          background: isDark
            ? `radial-gradient(ellipse 55% 60% at 50% 0%, ${ORANGE}33 0%, ${ORANGE}1a 25%, ${ORANGE}0d 45%, transparent 75%)`
            : `radial-gradient(ellipse 55% 60% at 50% 0%, ${ORANGE}40 0%, ${ORANGE}26 25%, ${ORANGE}12 45%, transparent 75%)`,
        }}
      />
    </div>
  );
}

// ---- Splash ----

function Splash({ isDark }) {
  return (
    <div className="relative min-h-screen flex flex-col items-center justify-center gap-4 overflow-hidden">
      <style>{`
        @keyframes splashWash {
          0% { opacity: 1; }
          70% { opacity: 1; }
          100% { opacity: 0; }
        }
        @keyframes splashGlow {
          0% { opacity: 0; transform: translateX(-50%) scale(0.3); }
          40% { opacity: 1; transform: translateX(-50%) scale(1.3); }
          60% { transform: translateX(-50%) scale(0.95); }
          80% { transform: translateX(-50%) scale(1.1); }
          100% { opacity: 1; transform: translateX(-50%) scale(1); }
        }
        @keyframes splashLogo {
          0% { opacity: 0; transform: scale(0) rotate(-45deg); }
          50% { opacity: 1; transform: scale(1.4) rotate(10deg); }
          70% { transform: scale(0.85) rotate(-4deg); }
          85% { transform: scale(1.1) rotate(2deg); }
          100% { opacity: 1; transform: scale(1) rotate(0deg); }
        }
        @keyframes splashText {
          0% { opacity: 0; transform: translateY(20px) scale(0.9); }
          100% { opacity: 1; transform: translateY(0) scale(1); }
        }
      `}</style>

      {/* Full-screen color flash that wipes away */}
      <div
        aria-hidden="true"
        className="absolute inset-0"
        style={{
          zIndex: 1,
          backgroundColor: ORANGE,
          animation: "splashWash 0.9s ease-in forwards",
        }}
      />

      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 overflow-hidden"
        style={{ zIndex: 0 }}
      >
        <div
          style={{
            position: "absolute",
            top: 0,
            left: "50%",
            width: "130%",
            maxWidth: "1000px",
            height: "750px",
            background: `radial-gradient(ellipse 55% 60% at 50% 0%, ${ORANGE}66 0%, ${ORANGE}38 25%, ${ORANGE}1a 45%, transparent 75%)`,
            animation: "splashGlow 1.3s cubic-bezier(0.34,1.56,0.64,1) 0.7s both",
          }}
        />
      </div>

      <div
        className="relative z-10 h-20 w-20 rounded-2xl flex items-center justify-center"
        style={{ backgroundColor: "#fff", animation: "splashLogo 0.9s cubic-bezier(0.34,1.56,0.64,1) 0.5s both" }}
      >
        <Zap size={36} style={{ color: ORANGE }} />
      </div>
      <h1
        className={`relative z-10 text-4xl font-bold tracking-tight ${isDark ? "text-white" : "text-neutral-900"}`}
        style={{ animation: "splashText 0.6s ease-out 1.1s both" }}
      >
        Sherd
      </h1>
    </div>
  );
}

// ---- Toggle: power on as client before entering the mesh ----

function TogglePage({ isDark, onEnabled }) {
  const [on, setOn] = useState(false);

  return (
    <div className="relative min-h-screen flex flex-col items-center justify-center px-6 overflow-hidden">
      <GlowBackground isDark={isDark} />

      <div className="relative z-10 flex flex-col items-center">
        <div className="h-12 w-12 rounded-xl flex items-center justify-center mb-4" style={{ backgroundColor: ORANGE }}>
          <Network size={22} className="text-white" />
        </div>
        <h1 className={`text-xl font-semibold mb-1 ${isDark ? "text-white" : "text-neutral-900"}`}>
          Power on as client
        </h1>
        <p className={`text-sm mb-8 text-center max-w-xs ${isDark ? "text-neutral-400" : "text-neutral-500"}`}>
          Turn this on to join the mesh. Your commands will route to whichever
          node picks up the task.
        </p>

        <button
          onClick={() => setOn(!on)}
          className="w-16 h-9 rounded-full flex items-center px-1 transition-colors"
          style={{ backgroundColor: on ? ORANGE : isDark ? "#404040" : "#e5e5e5" }}
        >
          <div
            className="h-7 w-7 rounded-full bg-white shadow transition-transform"
            style={{ transform: on ? "translateX(28px)" : "translateX(0px)" }}
          />
        </button>
        <p className={`text-xs mt-3 ${isDark ? "text-neutral-500" : "text-neutral-400"}`}>
          {on ? "On" : "Off"}
        </p>

        <button
          disabled={!on}
          onClick={onEnabled}
          className="mt-10 w-full max-w-xs text-sm font-medium rounded-xl py-3 transition-colors"
          style={{
            backgroundColor: on ? ORANGE : isDark ? "#262626" : "#f0f0f0",
            color: on ? "#fff" : isDark ? "#737373" : "#a3a3a3",
          }}
        >
          Continue
        </button>
      </div>
    </div>
  );
}

// ---- Mesh client: status, and a running feed of tasks + which node picked them up ----

function MeshClient({ isDark, user, authMethod, onLogout }) {
  const [tasks, setTasks] = useState([
    { id: 1, cmd: "npm run build", node: "Node 4", status: "done" },
    { id: 2, cmd: "python train.py --epochs 10", node: "Node 2", status: "running" },
  ]);
  const [nodes, setNodes] = useState([]);

  useEffect(() => {
    connectToMesh();
    listNodes().then(setNodes);
  }, []);

  const simulateTask = () => {
    runSimulatedTask((update) => {
      setTasks((prev) => {
        const exists = prev.some((t) => t.id === update.id);
        if (exists) return prev.map((t) => (t.id === update.id ? update : t));
        return [update, ...prev];
      });
    });
  };

  const cardBg = isDark ? "bg-neutral-900" : "bg-white";
  const cardBorder = isDark ? "border-neutral-800" : "border-neutral-100";
  const heading = isDark ? "text-white" : "text-neutral-900";
  const muted = isDark ? "text-neutral-500" : "text-neutral-400";
  const subMuted = isDark ? "text-neutral-400" : "text-neutral-500";

  return (
    <div className="relative min-h-screen overflow-hidden">
      <GlowBackground isDark={isDark} />

      <div className="relative z-10 max-w-lg mx-auto px-6 py-6">
        {/* Authenticated identity + logout */}
        <div className="flex items-center justify-between mb-3">
          <span className={`text-xs ${muted}`}>
            {authMethod?.walletAddress ? (
              <>
                Signed in with Solana wallet{" "}
                <span className={subMuted}>
                  {authMethod.walletAddress.slice(0, 4)}…{authMethod.walletAddress.slice(-4)}
                </span>{" "}
                <span className={muted}>
                  ({authMethod.kind === "phantom" ? "Phantom" : "demo keypair"})
                </span>
              </>
            ) : (
              <>
                Authenticated as{" "}
                <span className={subMuted}>{user?.email || user?.id || "unknown user"}</span>
              </>
            )}
          </span>
          <button
            onClick={onLogout}
            className={`flex items-center gap-1 text-xs ${muted} hover:${heading}`}
          >
            <LogOut size={12} /> Log out
          </button>
        </div>

        {/* Status header */}
        <div className="flex items-center justify-between mb-5">
          <div className="flex items-center gap-2">
            <Circle size={9} fill={ORANGE} color={ORANGE} />
            <span className={`text-sm font-medium ${heading}`}>Connected to mesh</span>
          </div>
          <span className={`text-xs ${muted}`}>247 nodes online</span>
        </div>

        {/* Explainer card */}
        <div
          className="rounded-xl px-4 py-3 mb-5 flex items-start gap-3"
          style={{ backgroundColor: isDark ? "#3a2620" : LIGHT_ORANGE }}
        >
          <Terminal size={16} style={{ color: ORANGE }} className="mt-0.5" />
          <p className={`text-xs leading-relaxed ${isDark ? "text-neutral-300" : "text-neutral-700"}`}>
            Go back to your terminal or editor as usual. Commands you run will
            be picked up and executed by an available node in the mesh, then
            results are sent back to you here. Node listings, task activity,
            and "picked up by" attribution below are demo/mock data — there is
            no real P2P network wired up yet.
          </p>
        </div>

        {/* Node price listing */}
        <h2 className={`text-sm font-medium mb-2 ${heading}`}>Nodes in the mesh</h2>
        <div className="space-y-2 mb-5">
          {nodes.map((n) => (
            <div
              key={n.id}
              className={`flex items-center justify-between border-2 rounded-xl px-4 py-2.5 ${cardBg} ${cardBorder}`}
            >
              <div className="flex items-center gap-3">
                <div
                  className="h-8 w-8 rounded-lg flex items-center justify-center text-white text-xs font-medium"
                  style={{ backgroundColor: ORANGE }}
                >
                  {n.id}
                </div>
                <span className={`text-sm ${heading}`}>Node {n.id}</span>
              </div>
              <span className={`text-sm ${subMuted}`}>
                ${n.price}<span className={`text-xs ${muted}`}>/hr</span>
              </span>
            </div>
          ))}
        </div>

        {/* Task feed */}
        <h2 className={`text-sm font-medium mb-2 ${heading}`}>Task activity</h2>
        <div className="space-y-2 mb-5">
          {tasks.map((t) => (
            <div
              key={t.id}
              className={`flex items-center justify-between border-2 rounded-xl px-4 py-3 ${cardBg} ${cardBorder}`}
            >
              <div className="min-w-0">
                <code className={`text-xs block truncate ${heading}`}>{t.cmd}</code>
                <span className={`text-xs ${muted}`}>picked up by {t.node}</span>
              </div>
              {t.status === "running" ? (
                <span className="text-xs font-medium" style={{ color: ORANGE }}>
                  running…
                </span>
              ) : (
                <span className={`text-xs flex items-center gap-1 ${muted}`}>
                  <Check size={12} /> done
                </span>
              )}
            </div>
          ))}
        </div>

        <button
          onClick={simulateTask}
          className="w-full text-sm font-medium rounded-xl py-3 text-white"
          style={{ backgroundColor: ORANGE }}
        >
          Simulate a task
        </button>
      </div>
    </div>
  );
}
