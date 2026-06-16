import puppeteer from "puppeteer-core";
import { mkdir } from "node:fs/promises";

const BASE = process.env.BASE_URL ?? "http://localhost:8080";
const OUT = process.env.SCREENSHOT_DIR ?? "/opt/cursor/artifacts/screenshots";
const delay = (ms) => new Promise((r) => setTimeout(r, ms));

await mkdir(OUT, { recursive: true });

const browser = await puppeteer.launch({
  executablePath: "/usr/local/bin/google-chrome",
  headless: "new",
  args: ["--no-sandbox", "--disable-setuid-sandbox"],
});

const page = await browser.newPage();

async function shot(name, url, setup) {
  await page.setViewport({ width: 1280, height: 800 });
  await page.goto(`${BASE}${url}`, { waitUntil: "networkidle0", timeout: 30000 });
  if (setup) await setup(page);
  await delay(1500);
  await page.waitForFunction(
    () => !document.querySelector(".loading-screen"),
    { timeout: 10000 },
  ).catch(() => {});
  await page.screenshot({ path: `${OUT}/${name}.png`, fullPage: false });
  console.log(`saved ${name}.png`);
}

await shot("01-chats-main", "/chats/1");
await shot("02-hamburger-menu", "/chats/1", async (p) => {
  await p.click('button[aria-label="Open panels menu"]');
});
await shot("03-character-modal", "/chats/1/character");
await shot("04-variables-modal", "/chats/1/variables");
await shot("05-settings-page", "/settings");
await shot("06-stories-main", "/stories/1");
await shot("07-mobile-chats", "/chats/1", async (p) => {
  await p.setViewport({ width: 390, height: 844, isMobile: true });
});
await shot("08-mobile-character-modal", "/chats/1/character", async (p) => {
  await p.setViewport({ width: 390, height: 844, isMobile: true });
});

await browser.close();
console.log(`Screenshots written to ${OUT}`);
