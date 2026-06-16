import { expect, test } from "@playwright/test";

async function seedRunningChat(request: import("@playwright/test").APIRequestContext) {
  const response = await request.post("/api/e2e/seed-chat-running");
  expect(response.ok()).toBeTruthy();
  return (await response.json()) as { chat_id: number; expected_content: string };
}

async function completeChatJob(
  request: import("@playwright/test").APIRequestContext,
  chatId: number,
) {
  const response = await request.post(`/api/e2e/complete-chat-job/${chatId}`);
  expect(response.ok()).toBeTruthy();
}

async function installVisibilityShim(page: import("@playwright/test").Page) {
  await page.addInitScript(() => {
    let hidden = false;
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      get: () => (hidden ? "hidden" : "visible"),
    });
    Object.defineProperty(document, "hidden", {
      configurable: true,
      get: () => hidden,
    });
    (window as unknown as {
      __setTabHidden: (v: boolean) => void;
      __showFromBfcache: () => void;
    }).__setTabHidden = (v: boolean) => {
      hidden = v;
      document.dispatchEvent(new Event("visibilitychange"));
    };
    (window as unknown as { __showFromBfcache: () => void }).__showFromBfcache = () => {
      hidden = false;
      window.dispatchEvent(new PageTransitionEvent("pageshow", { persisted: true }));
    };
  });
}

async function setTabHidden(page: import("@playwright/test").Page, hidden: boolean) {
  await page.evaluate((value) => {
    (window as unknown as { __setTabHidden: (v: boolean) => void }).__setTabHidden(value);
  }, hidden);
}

test.describe("tab visibility resume", () => {
  test.beforeEach(async ({ page }) => {
    await installVisibilityShim(page);
  });

  test("chat shows completed content after returning from background", async ({
    page,
    request,
  }) => {
    const seed = await seedRunningChat(request);
    await page.goto(`/chats/${seed.chat_id}`);
    await expect(page.getByText("Still writing — more coming…")).toBeVisible();

    await setTabHidden(page, true);
    await completeChatJob(request, seed.chat_id);
    await setTabHidden(page, false);

    await expect(page.getByText(seed.expected_content)).toBeVisible();
    await expect(page.getByText("Still writing — more coming…")).toHaveCount(0);
  });

  test("chat continues showing generation while job still running after resume", async ({
    page,
    request,
  }) => {
    const seed = await seedRunningChat(request);
    await page.goto(`/chats/${seed.chat_id}`);
    await expect(page.getByText("Still writing — more coming…")).toBeVisible();

    await setTabHidden(page, true);
    await setTabHidden(page, false);

    await expect(page.getByText("Still writing — more coming…")).toBeVisible();
  });

  test("bfcache-style resume refetches completed content", async ({ page, request }) => {
    const seed = await seedRunningChat(request);
    await page.goto(`/chats/${seed.chat_id}`);
    await expect(page.getByText("Still writing — more coming…")).toBeVisible();

    await setTabHidden(page, true);
    await completeChatJob(request, seed.chat_id);

    await page.evaluate(() => {
      (window as unknown as { __showFromBfcache: () => void }).__showFromBfcache();
    });

    await expect(page.getByText(seed.expected_content)).toBeVisible({ timeout: 45_000 });
    await expect(page.getByText("Still writing — more coming…")).toHaveCount(0);
  });
});
