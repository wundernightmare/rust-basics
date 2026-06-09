import { expect, test } from "@playwright/test";

import { CONSUMER_URL, TASKS_URL, WITH_DEPS } from "../helpers/env";

// This suite drives the data-services vertical (Valkey + Kafka) and only runs
// when E2E_WITH_DEPS=1 — see fixtures/services.ts and `just e2e-deps`.
test.describe("tasks service", () => {
  test.skip(!WITH_DEPS, "needs Valkey + Kafka (run `just e2e-deps`)");

  test("readyz reports every dependency healthy @smoke", async ({ request }) => {
    const res = await request.get(`${TASKS_URL}/readyz`);
    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.status).toBe("ready");
    expect(body.checks).toMatchObject({ valkey: "ok", kafka: "ok" });
  });

  test("create → read → list → delete → 404", async ({ request }) => {
    const created = await request.post(`${TASKS_URL}/tasks`, { data: { title: "e2e task" } });
    expect(created.status()).toBe(201);
    const task = await created.json();
    expect(task.title).toBe("e2e task");
    expect(task.id).toBeTruthy();

    const read = await request.get(`${TASKS_URL}/tasks/${task.id}`);
    expect(read.status()).toBe(200);
    expect((await read.json()).id).toBe(task.id);

    const list = await request.get(`${TASKS_URL}/tasks`);
    expect(list.status()).toBe(200);
    const ids = (await list.json()).tasks.map((t: { id: string }) => t.id);
    expect(ids).toContain(task.id);

    const del = await request.delete(`${TASKS_URL}/tasks/${task.id}`);
    expect(del.status()).toBe(204);

    const missing = await request.get(`${TASKS_URL}/tasks/${task.id}`);
    expect(missing.status()).toBe(404);
    expect(missing.headers()["content-type"]).toContain("application/problem+json");
    expect((await missing.json()).code).toBe("task_not_found");
  });

  test("empty title is rejected with a 400 problem", async ({ request }) => {
    const res = await request.post(`${TASKS_URL}/tasks`, { data: { title: "" } });
    expect(res.status()).toBe(400);
    expect(res.headers()["content-type"]).toContain("application/problem+json");
    expect((await res.json()).code).toBe("empty_title");
  });

  test("the consumer drains the task.created event", async ({ request }) => {
    const before = await consumedTotal(request);

    const created = await request.post(`${TASKS_URL}/tasks`, {
      data: { title: "for the consumer" },
    });
    expect(created.status()).toBe(201);

    await expect
      .poll(async () => consumedTotal(request), { timeout: 15_000, intervals: [250] })
      .toBeGreaterThan(before);
  });
});

/** Read consumer_tasks_consumed_total from the consumer's /metrics endpoint. */
async function consumedTotal(
  request: import("@playwright/test").APIRequestContext,
): Promise<number> {
  const res = await request.get(`${CONSUMER_URL}/metrics`);
  if (!res.ok()) return 0;
  const line = (await res.text())
    .split("\n")
    .find((l) => l.startsWith("consumer_tasks_consumed_total"));
  return line ? Number(line.split(/\s+/)[1]) : 0;
}
