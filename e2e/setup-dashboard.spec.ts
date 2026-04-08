import { expect, test } from "@playwright/test";

const PENGINE_API = "http://127.0.0.1:21516";

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
async function mockPengineApi(page: import("@playwright/test").Page) {
  await page.route(`${PENGINE_API}/v1/health`, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        status: "ok",
        bot_connected: true,
        bot_username: "TestPengineBot",
      }),
    });
  });

  await page.route(`${PENGINE_API}/v1/connect`, async (route) => {
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
  test("redirects dashboard to setup when disconnected", async ({ page }) => {
    await page.goto("/dashboard");
    await expect(page.getByTestId("app-ready")).toBeVisible();

    await expect(page).toHaveURL(/\/setup$/);
    await expect(
      page.getByRole("heading", { name: "Create your Telegram bot", exact: true }),
    ).toBeVisible();
  });

  test("walks all setup wizard steps and opens dashboard", async ({ page }) => {
    await mockPengineApi(page);
    await page.goto("/setup");
    await expect(page.getByTestId("app-ready")).toBeVisible();

    // Step 1: Create bot
    await expect(
      page.getByRole("heading", { name: "Create your Telegram bot", exact: true }),
    ).toBeVisible();
    await page.getByLabel("Bot token").fill("12345678:abcdefghijklmnopqrstuvwxyzABCDE12345");
    await page.getByRole("button", { name: "Continue" }).click();

    // Step 2: Install Ollama
    await expect(page.getByRole("heading", { name: "Install Ollama", exact: true })).toBeVisible();
    await page.getByTestId("ollama-acknowledge").click();
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
    await mockPengineApi(page);
    await page.addInitScript((state) => {
      window.localStorage.setItem("pengine-device-session", JSON.stringify(state));
    }, CONNECTED_STORAGE_STATE);

    await page.goto("/dashboard");
    await expect(page.getByTestId("app-ready")).toBeVisible();

    await expect(page).toHaveURL(/\/dashboard$/);
    await expect(page.getByText("1 connected device")).toBeVisible();
  });
});
