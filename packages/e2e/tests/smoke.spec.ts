import { expect, test, type Page } from "@playwright/test";
import { Authenticator } from "./totp";

// One flow through the real app, in order: the database starts empty.
test.describe.configure({ mode: "serial" });

const email = "admin@example.test";
const password = "correct horse battery";

/** The password step; `then` is what it leads to: the app, or the code step. */
async function signIn(page: Page, then: "app" | "code" = "app") {
  await page.goto("/login");
  await page.getByPlaceholder("Email").fill(email);
  await page.getByPlaceholder(/^Password/).fill(password);
  await page.getByRole("button", { name: "Sign in" }).click();
  if (then === "app") {
    await expect(page.getByRole("button", { name: email })).toBeVisible();
  } else {
    await expect(page.getByRole("heading", { name: "Two-factor authentication" })).toBeVisible();
  }
}

async function signOut(page: Page) {
  await page.getByRole("button", { name: email }).click();
  await page.getByRole("menuitem", { name: "Sign out" }).click();
  await expect(page).toHaveURL(/\/login$/);
}

test("first setup creates the admin and signs in", async ({ page }) => {
  await page.goto("/");
  await expect(page).toHaveURL(/\/setup$/);
  await page.getByPlaceholder("Email").fill(email);
  await page.getByPlaceholder(/^Password/).fill(password);
  await page.getByRole("button", { name: "Create admin" }).click();
  await expect(page.getByRole("button", { name: email })).toBeVisible();
});

test("signing out and in", async ({ page }) => {
  await signIn(page);
  await signOut(page);
  // A wrong password does not get in.
  await page.getByPlaceholder("Email").fill(email);
  await page.getByPlaceholder(/^Password/).fill("wrong password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("alert")).toBeVisible();
  await expect(page).toHaveURL(/\/login$/);
});

/** The authenticator app set up in the two-factor test, used again by the later ones. */
let app: Authenticator | undefined;

test("two-factor authentication: set up, then sign in with a code", async ({ page }) => {
  await signIn(page);
  await page.goto("/settings");
  await page.getByRole("button", { name: "Set up" }).click();
  app = new Authenticator((await page.locator("code").first().textContent())!.trim());
  await page.getByPlaceholder("Code from the app").fill(await app.code());
  await page.getByRole("button", { name: "Turn on" }).click();
  // The recovery codes, shown once.
  const codes = page.locator("li").filter({ hasText: /^[a-z0-9]{5}-[a-z0-9]{5}$/ });
  await expect(codes).toHaveCount(10);
  const recoveryCode = (await codes.first().textContent())!.trim();
  await page.getByRole("button", { name: "Done" }).click();
  await signOut(page);

  // The password alone is not enough now: a recovery code finishes it (the
  // app's code of this step was used up by turning it on).
  await signIn(page, "code");
  await page.getByRole("button", { name: "Use a recovery code" }).click();
  await page.getByPlaceholder("xxxxx-xxxxx").fill(recoveryCode);
  await page.getByRole("button", { name: "Verify" }).click();
  await expect(page.getByRole("button", { name: email })).toBeVisible();
});

test("a site, a page view, and the dashboard shows it", async ({ page }) => {
  // The second step with the app's code (the authenticator waits for a fresh
  // one: each step's code is accepted once).
  await signIn(page, "code");
  await page.getByPlaceholder("123456").fill(await app!.code());
  await page.getByRole("button", { name: "Verify" }).click();
  await expect(page.getByRole("button", { name: email })).toBeVisible();

  await page.goto("/sites");
  await page.getByPlaceholder("blog.example.com").fill("example.org");
  await page.getByRole("button", { name: "Add site" }).click();
  await expect(page.getByText("example.org").first()).toBeVisible();

  // A visitor's page view, as the tracker sends it.
  const hit = await page.request.post("/api/collect", {
    headers: {
      "content-type": "text/plain",
      "user-agent":
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36",
    },
    data: JSON.stringify({ type: "pageview", host: "example.org", path: "/hello" }),
  });
  expect(hit.status()).toBe(204);

  await page.goto("/");
  await expect(page.getByText("/hello")).toBeVisible();
});
