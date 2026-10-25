// Renders demo.html into images/demo.mp4 and images/demo.gif, frame by frame.
// Needs Google Chrome, ffmpeg and Playwright: NODE_PATH=$(npm root -g)/@playwright/mcp/node_modules node docs/media-src/render-demo.mjs
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createRequire } from 'node:module';

const { chromium } = createRequire(import.meta.url)('playwright');
const root = resolve(import.meta.dirname, '../..');
const FPS = 30;
const frames = mkdtempSync(join(tmpdir(), 'magicinput-demo-'));
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 }, deviceScaleFactor: 1 });
await page.goto('file://' + join(root, 'docs/media-src/demo.html'));
const duration = await page.evaluate(() => window.DURATION);
for (let i = 0; i < duration * FPS; i++) {
  await page.evaluate(t => window.frame(t), i / FPS);
  await page.screenshot({ path: join(frames, `f${String(i).padStart(4, '0')}.png`) });
}
await browser.close();
const input = ['-framerate', String(FPS), '-i', join(frames, 'f%04d.png')];
execFileSync('ffmpeg', ['-y', '-loglevel', 'error', ...input, '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '20', '-movflags', '+faststart', join(root, 'images/demo.mp4')]);
// The GIF (for the README): smaller and slower, with a palette made for these frames.
execFileSync('ffmpeg', ['-y', '-loglevel', 'error', ...input, '-vf',
  'fps=15,scale=880:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle',
  join(root, 'images/demo.gif')]);
rmSync(frames, { recursive: true });
console.log('ok');
