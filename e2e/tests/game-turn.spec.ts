import { expect, test } from "@playwright/test";

async function seedRunningGame(request: import("@playwright/test").APIRequestContext) {
  const response = await request.post("/api/e2e/seed-game-running");
  expect(response.ok()).toBeTruthy();
  return (await response.json()) as { game_id: number; expected_content: string };
}

async function completeGameJob(
  request: import("@playwright/test").APIRequestContext,
  gameId: number,
) {
  const response = await request.post(`/api/e2e/complete-game-job/${gameId}`);
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

test.describe("game turn tab visibility resume", () => {
  test.beforeEach(async ({ page }) => {
    await installVisibilityShim(page);
  });

  test("game shows completed turn prose after returning from background", async ({
    page,
    request,
  }) => {
    const seed = await seedRunningGame(request);
    await expect
      .poll(async () => {
        const response = await request.get(`/api/games/${seed.game_id}`);
        if (!response.ok()) return null;
        const body = (await response.json()) as {
          game: { active_job?: { status: string } | null };
        };
        return body.game.active_job?.status ?? null;
      })
      .toBe("running");

    await page.goto(`/games/${seed.game_id}`);
    await expect(page.getByText("I pick the lock.")).toBeVisible();
    await expect(
      page.locator("aside, main").getByText("running structured agent…").first(),
    ).toBeVisible();

    await setTabHidden(page, true);
    await completeGameJob(request, seed.game_id);
    await setTabHidden(page, false);

    const main = page.locator("main");
    await expect(main.getByText(seed.expected_content)).toBeVisible();
    await expect(main.getByText("running structured agent…")).toHaveCount(0);
  });
});
