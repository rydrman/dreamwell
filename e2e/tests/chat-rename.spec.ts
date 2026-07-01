import { expect, test } from "@playwright/test";

async function seedChat(request: import("@playwright/test").APIRequestContext) {
  const response = await request.post("/api/e2e/seed-chat-running");
  expect(response.ok()).toBeTruthy();
  return (await response.json()) as { chat_id: number };
}

test.describe("chat rename", () => {
  test("renamed chat stays in chat list", async ({ page, request }) => {
    const seed = await seedChat(request);
    await page.goto(`/chats/${seed.chat_id}`);

    await page.locator(".content-header .title-editable").click();
    const input = page.locator(".content-header input.header-title");
    await expect(input).toBeVisible();
    await input.fill("Renamed chat");
    await input.press("Enter");

    await expect(page.locator(".content-header .title-editable")).toHaveText("Renamed chat");

    await page.goto("/chats");
    await expect(page.locator(".main-list .chat-item-title")).toHaveText("Renamed chat");

    const listResponse = await request.get("/api/chats");
    expect(listResponse.ok()).toBeTruthy();
    const chats = (await listResponse.json()) as Array<{ id: number; title: string }>;
    expect(chats.some((chat) => chat.id === seed.chat_id && chat.title === "Renamed chat")).toBe(
      true,
    );
  });
});
