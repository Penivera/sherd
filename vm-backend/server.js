import 'dotenv/config';
import express from 'express';
import cors from 'cors';
import fs from 'fs';
import { DesktopClient } from '@solarisdk/desktop';

const app = express();
app.use(cors());

const desktops = new DesktopClient({
  apiKey: process.env.SOLARI_API_KEY,
  baseUrl: 'https://api.getsolari.com',
});

// We remember our VM's ID here, so restarting the server doesn't
// blindly create a brand new VM every time (and hit the 1-slot limit).
const SESSION_FILE = './desktop-session.json';

let activeDesktop = null;

function saveSession(desktop) {
  fs.writeFileSync(SESSION_FILE, JSON.stringify({
    id: desktop.id || desktop.sessionId,
  }));
}

function clearSession() {
  if (fs.existsSync(SESSION_FILE)) fs.unlinkSync(SESSION_FILE);
}

// Makes sure we have a real, current streamUrl for a desktop object,
// whether it just came from create() (which includes it already) or
// from get() (which may not).
async function ensureStreamUrl(vm) {
  if (vm.streamUrl) return vm.streamUrl;

  await vm.connect();
  const { streamUrl } = await vm.stream.start();
  return streamUrl;
}

async function loadExistingDesktop() {
  if (!fs.existsSync(SESSION_FILE)) return null;

  try {
    const saved = JSON.parse(fs.readFileSync(SESSION_FILE, 'utf-8'));
    console.log('Found a saved VM ID, checking status:', saved.id);

    const vm = await desktops.get(saved.id);

    if (vm.status === 'gone' || vm.status === 'releasing') {
      console.log('Saved VM is no longer usable (status:', vm.status, ') — will create a new one.');
      clearSession();
      return null;
    }

    if (vm.status === 'paused') {
      console.log('Saved VM is paused — resuming it...');
      await vm.resume();
    }

    console.log('Reusing existing VM.');
    return vm;
  } catch (err) {
    // Only give up on the saved VM if Solari clearly says it no longer
    // exists (a 404 / "not found" style error). Anything else — a
    // dropped connection, a timeout, a temporary network blip — should
    // NOT wipe the saved session, since the VM is probably still alive
    // and we don't want to accidentally try to create a duplicate.
    const status = err.status || err.statusCode;
    const message = (err.message || '').toLowerCase();
    const looksGone = status === 404 || message.includes('not found');

    if (looksGone) {
      console.log('Saved VM genuinely no longer exists — will create a new one.');
      clearSession();
      return null;
    }

    console.error('Could not reach Solari to check the saved VM (network issue?).', err.message);
    throw new Error('Could not verify existing desktop — try again once your connection is stable.');
  }
}

app.get('/api/desktop', async (req, res) => {
  try {
    if (!activeDesktop) {
      activeDesktop = await loadExistingDesktop();
    }

    if (!activeDesktop) {
      console.log('Creating a new desktop...');
      activeDesktop = await desktops.create({
        template: 'default',
        resolution: '1280x720',
        cpu: 2,
        memMb: 4096,
        timeoutMs: 15 * 60 * 1000,
        lifecycle: { onTimeout: 'pause' },
      });
      saveSession(activeDesktop);
      console.log('Desktop created.');
    }

    const streamUrl = await ensureStreamUrl(activeDesktop);
    res.json({ streamUrl });
  } catch (err) {
    console.error('Failed to create/get desktop:', err);
    res.status(500).json({ error: 'Failed to start desktop' });
  }
});

const PORT = 3001;
app.listen(PORT, () => {
  console.log(`Backend running at http://127.0.0.1:${PORT}`);
});
