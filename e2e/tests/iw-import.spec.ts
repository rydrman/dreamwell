import fs from "node:fs";
import path from "node:path";

import { expect, test } from "@playwright/test";

const fixturePath = path.join(
  process.cwd(),
  "../crates/server/tests/fixtures/iw_board_game_scenario.json",
);

test.describe("IW scenario import", () => {
  test("imports board game scenario with rules blocks", async ({ request }) => {
    const body = fs.readFileSync(fixturePath);
    const response = await request.post("/api/scenarios/import-iw", {
      multipart: {
        file: {
          name: "iw_board_game_scenario.json",
          mimeType: "application/json",
          buffer: body,
        },
      },
    });
    expect(response.ok()).toBeTruthy();
    const data = (await response.json()) as {
      scenario: { id: number; title: string; rules_blocks: { name: string }[] };
      source: string;
    };
    expect(data.source).toBe("iw");
    expect(data.scenario.title).toBe("Crystal Quest");
    expect(data.scenario.rules_blocks.length).toBeGreaterThan(0);
    expect(
      data.scenario.rules_blocks.some((b) => b.name === "Game Mechanics"),
    ).toBeTruthy();

    const fetched = await request.get(`/api/scenarios/${data.scenario.id}`);
    expect(fetched.ok()).toBeTruthy();
    const scenario = (await fetched.json()) as {
      cast: unknown[];
      trait_defs: { name: string }[];
    };
    expect(scenario.cast.length).toBe(8);
    expect(scenario.trait_defs.length).toBe(5);
  });
});
