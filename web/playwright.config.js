import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './tests',
  timeout: 30000,
  expect: {
    timeout: 5000,
  },
  fullyParallel: false,
  reporter: 'list',
  use: {
    baseURL: process.env.BASE_URL || 'http://localhost:8080',
    trace: 'on-first-retry',
    headless: true,
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
    },
  ],
  webServer: {
    command: 'cargo run',
    cwd: '../',
    url: 'http://127.0.0.1:8080/api/v1/auth/status',
    reuseExistingServer: !process.env.CI,
    timeout: 60000,
    env: {
      PORT: '8080',
      HOST: '127.0.0.1',
      DATABASE_URL: 'sqlite://playwright_test.db?mode=rwc',
      DEV_LOGIN_ENABLED: 'true',
      OIDC_ENABLED: 'false',
      APP_URL: 'http://127.0.0.1:8080',
    },
  },
});
