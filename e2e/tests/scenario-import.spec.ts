import fs from "node:fs";
import path from "node:path";

import { expect, test } from "@playwright/test";

const fixturePath = path.join(
  process.cwd(),
  "../crates/server/tests/fixtures/sample_scenario_export.json",
);

test.describe("Scenario JSON import/export", () => {
  test("imports native scenario export format", async ({ request }) => {
    const body = fs.readFileSync(fixturePath);
    const response = await request.post("/api/scenarios/import", {
      multipart: {
        file: {
          name: "sample_scenario_export.json",
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
    expect(data.source).toBe("scenario");
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
      game_elements: { decks: { id: string }[] };
    };
    expect(scenario.cast.length).toBe(1);
    expect(scenario.trait_defs.length).toBe(2);
    expect(scenario.game_elements.decks.length).toBe(1);
  });

  test("exports scenario as native JSON", async ({ request }) => {
    const body = fs.readFileSync(fixturePath);
    const imported = await request.post("/api/scenarios/import", {
      multipart: {
        file: {
          name: "sample_scenario_export.json",
          mimeType: "application/json",
          buffer: body,
        },
      },
    });
    expect(imported.ok()).toBeTruthy();
    const { scenario } = (await imported.json()) as {
      scenario: { id: number; title: string };
    };

    const exported = await request.get(
      `/api/scenarios/${scenario.id}/export`,
    );
    expect(exported.ok()).toBeTruthy();
    const payload = (await exported.json()) as {
      format: string;
      title: string;
      rules_blocks: { name: string }[];
    };
    expect(payload.format).toBe("dreamwell.scenario.v1");
    expect(payload.title).toBe("Crystal Quest");
    expect(
      payload.rules_blocks.some((b) => b.name === "Cards and probabilities"),
    ).toBeTruthy();
  });
});
