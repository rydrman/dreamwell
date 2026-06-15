import puppeteer from "puppeteer-core";
import { mkdir } from "node:fs/promises";

const BASE = process.env.BASE_URL ?? "http://localhost:8080";
const OUT =
  process.env.SCREENSHOT_DIR ?? "/opt/cursor/artifacts/screenshots/story-textareas";
const delay = (ms) => new Promise((r) => setTimeout(r, ms));

const LONG_TEXT = Array.from(
  { length: 12 },
  (_, i) =>
    `Line ${i + 1}: The harbor fog lifted slowly, revealing rooftops and the old clock tower.`,
).join("\n");

const STREAM_CHUNK = "Another sentence arrived from the generator while the field was locked.\n";

await mkdir(OUT, { recursive: true });

const browser = await puppeteer.launch({
  executablePath: "/usr/local/bin/google-chrome",
  headless: "new",
  args: ["--no-sandbox", "--disable-setuid-sandbox"],
});

const page = await browser.newPage();

async function measureTextareas(selector) {
  return page.$$eval(selector, (nodes) =>
    nodes.map((node) => {
      const el = /** @type {HTMLTextAreaElement} */ (node);
      const style = window.getComputedStyle(el);
      return {
        valueLines: el.value.split("\n").length,
        clientHeight: el.clientHeight,
        scrollHeight: el.scrollHeight,
        overflowY: style.overflowY,
        clipped: el.scrollHeight > el.clientHeight + 2,
      };
    }),
  );
}

async function shot(name, setup) {
  await page.setViewport({ width: 1280, height: 900 });
  await page.goto(`${BASE}/stories/1`, {
    waitUntil: "networkidle0",
    timeout: 60000,
  });
  await delay(2000);
  if (setup) await setup(page);
  await delay(800);
  await page.screenshot({ path: `${OUT}/${name}.png`, fullPage: false });
  console.log(`saved ${name}.png`);
}

await shot("01-story-outline-default", async (p) => {
  const buttons = await p.$$("button");
  for (const btn of buttons) {
    const text = await p.evaluate((el) => el.textContent, btn);
    if (text?.trim() === "Outline") {
      await btn.click();
      break;
    }
  }
  await delay(500);
});

await shot("02-story-basics-long-premise", async (p) => {
  await p.click("#story-basics .story-block-header");
  await delay(600);
  await p.evaluate((text) => {
    const textarea = document.querySelector(
      "#story-basics textarea",
    );
    if (!textarea) return;
    textarea.value = text;
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  }, LONG_TEXT);
  await delay(800);
});

async function openLastBeat(p) {
  const headers = await p.$$(".story-block-header.indented");
  if (headers.length > 0) {
    await headers[headers.length - 1].click();
  }
  await delay(1000);
}

await shot("03-story-beat-prose-long", async (p) => {
  await openLastBeat(p);
  await p.evaluate((text) => {
    const prose = document.querySelector(".prose-editor");
    if (!prose) return;
    prose.value = text;
    prose.dispatchEvent(new Event("input", { bubbles: true }));
  }, LONG_TEXT);
  await delay(800);
});

await shot("04-story-beat-prose-streaming-sim", async (p) => {
  await openLastBeat(p);
  await p.evaluate((text) => {
    const prose = document.querySelector(".prose-editor");
    if (!prose) return;
    prose.value = text;
    prose.dispatchEvent(new Event("input", { bubbles: true }));
  }, LONG_TEXT);
  await delay(400);
  await p.evaluate((chunk) => {
    const prose = document.querySelector(".prose-editor");
    if (!prose) return;
    prose.readOnly = true;
    const next = prose.value + chunk.repeat(6);
    prose.value = next;
    prose.dispatchEvent(new Event("input", { bubbles: true }));
  }, STREAM_CHUNK);
  await delay(800);

  const metrics = await measureTextareas(".story-editor textarea");
  console.log("beat textarea metrics:", JSON.stringify(metrics, null, 2));
  const clipped = metrics.filter((m) => m.clipped);
  if (clipped.length > 0) {
    console.error("FAIL: clipped textareas detected", clipped);
    process.exitCode = 1;
  } else if (metrics.length === 0) {
    console.error("FAIL: no story textareas found to measure");
    process.exitCode = 1;
  } else {
    console.log("PASS: all measured textareas fit their content");
  }
});

await browser.close();
console.log(`Screenshots written to ${OUT}`);
