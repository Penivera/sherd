import 'dotenv/config';
import { DesktopClient } from '@solarisdk/desktop';

const desktops = new DesktopClient({
  apiKey: process.env.SOLARI_API_KEY,
  baseUrl: 'https://api.getsolari.com',
});

const desktop = await desktops.create({
  template: 'default',
  resolution: '1280x720',
  cpu: 2,
  memMb: 4096,
  timeoutMs: 15 * 60 * 1000,
  lifecycle: { onTimeout: 'pause' },
});

console.log('Desktop created.');
console.log('Watch it live at:', desktop.streamUrl);