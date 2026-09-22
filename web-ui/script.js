import RFB from 'https://cdn.jsdelivr.net/npm/@novnc/novnc@1.4.0/core/rfb.js';

const BACKEND_URL = 'http://127.0.0.1:3001/api/desktop';

const screenFrame = document.getElementById('screenFrame');
const placeholder = document.getElementById('placeholder');
const statusDot = document.getElementById('statusDot');

let rfb = null;

async function connectToDesktop() {
  placeholder.textContent = 'Starting desktop…';

  try {
    const response = await fetch(BACKEND_URL);
    const data = await response.json();

    if (!data.streamUrl) {
      throw new Error('No streamUrl returned from backend');
    }

    rfb = new RFB(screenFrame, data.streamUrl);
    rfb.scaleViewport = true;

    rfb.addEventListener('connect', () => {
      statusDot.classList.add('connected');
      statusDot.title = 'connected';
      placeholder.style.display = 'none';
    });

    rfb.addEventListener('disconnect', () => {
      statusDot.classList.remove('connected');
      statusDot.title = 'disconnected';
      placeholder.style.display = 'block';
      placeholder.textContent = 'Disconnected.';
    });
  } catch (err) {
    console.error('Failed to connect to desktop:', err);
    placeholder.textContent = 'Could not reach the backend, ensure is server.js running!';
  }
}

connectToDesktop();
