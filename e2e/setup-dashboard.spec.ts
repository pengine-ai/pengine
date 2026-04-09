import { expect, test } from "@playwright/test";
import { OLLAMA_API_BASE, PENGINE_API_BASE } from "../src/shared/api/config";

const CONNECTED_STORAGE_STATE = {
  state: {
    isDeviceConnected: true,
    botUsername: "TestPengineBot",
    botId: "12345678",
  },
  version: 0,
};

/**
 * Mock the loopback Pengine API so E2E tests work without a running
 * Tauri backend. Each test that needs it calls this helper.
 */
async function mockApis(page: import("@playwright/test").Page) {
  // Ollama mocks
  await page.route(`${OLLAMA_API_BASE}/api/ps`, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        models: [{ name: "qwen3-coder:30b", model: "qwen3-coder:30b" }],
      }),
    });
  });
  await page.route(`${OLLAMA_API_BASE}/api/tags`, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        models: [{ name: "qwen3-coder:30b", model: "qwen3-coder:30b" }],
      }),
    });
  });

  // Pengine mocks
  await page.route(`${PENGINE_API_BASE}/v1/health`, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        status: "ok",
        bot_connected: true,
        bot_username: "TestPengineBot",
        bot_id: "12345678",
      }),
    });
  });

  await page.route(
    (url) => url.href.startsWith(`${PENGINE_API_BASE}/v1/logs`),
    async (route) => {
      await route.fulfill({
        status: 200,
        contentType: "text/event-stream",
        body:
          'data: {"timestamp":"12:00:00","kind":"ok","message":"mock log"}\n\n',
      });
    },
  );

  await page.route(`${PENGINE_API_BASE}/v1/connect`, async (route) => {
    if (route.request().method() === "POST") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          status: "connected",
          bot_id: "12345678",
          bot_username: "TestPengineBot",
        }),
      });
    } else if (route.request().method() === "DELETE") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({ status: "disconnected" }),
      });
    } else {
      await route.continue();
    }
  });
}

test.describe("setup to dashboard flow", () => {
  test("shows 'no device' on dashboard when disconnected", async ({ page }) => {
    await page.goto("/dashboard");
    await expect(page.getByTestId("app-ready")).toBeVisible();

    await expect(page).toHaveURL(/\/dashboard$/);
    await expect(page.getByText("No device connected")).toBeVisible();
    await expect(page.getByRole("link", { name: "Go to setup" })).toBeVisible();
  });

  test("walks all setup wizard steps and opens dashboard", async ({ page }) => {
    await mockApis(page);
    await page.goto("/setup");
    await expect(page.getByTestId("app-ready")).toBeVisible();

    // Step 1: Create bot
    await expect(
      page.getByRole("heading", { name: "Create your Telegram bot", exact: true }),
    ).toBeVisible();
    await page.getByLabel("Bot token").fill("12345678:abcdefghijklmnopqrstuvwxyzABCDE12345");
    await page.getByRole("button", { name: "Continue" }).click();

    // Step 2: Install Ollama (auto-detected via mock)
    await expect(page.getByRole("heading", { name: "Install Ollama", exact: true })).toBeVisible();
    await expect(page.getByText("Ollama detected")).toBeVisible();
    await expect(page.getByText("Ready to continue.")).toBeVisible();
    await page.getByRole("button", { name: "Continue" }).click();

    // Step 3: Pengine local (health check auto-passes via mock)
    await expect(
      page.getByRole("heading", { name: "Start Pengine locally", exact: true }),
    ).toBeVisible();
    await expect(page.getByText("Pengine is running on localhost.")).toBeVisible();
    await page.getByRole("button", { name: "Continue" }).click();

    // Step 4: Connect
    await expect(
      page.getByRole("heading", { name: "Connect bot to Pengine", exact: true }),
    ).toBeVisible();
    await page.getByTestId("connect-to-pengine").click();
    await expect(page.getByText("Connected as @TestPengineBot")).toBeVisible();
    await page.getByRole("button", { name: "Open dashboard" }).click();

    await expect(page).toHaveURL(/\/dashboard$/);
    await expect(
      page.getByRole("heading", { name: "Connected device and running services" }),
    ).toBeVisible();
    await expect(page.getByText("Telegram gateway")).toBeVisible();
  });

  test("loads dashboard when device is already connected", async ({ page }) => {
    await mockApis(page);
    await page.addInitScript((state) => {
      window.localStorage.setItem("pengine-device-session", JSON.stringify(state));
    }, CONNECTED_STORAGE_STATE);

    await page.goto("/dashboard");
    await expect(page.getByTestId("app-ready")).toBeVisible();

    await expect(page).toHaveURL(/\/dashboard$/);
    await expect(page.getByText("1 connected device")).toBeVisible();
  });
});
