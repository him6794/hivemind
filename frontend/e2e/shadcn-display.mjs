import { expect } from '@playwright/test';

export async function waitForStableDisplay(page) {
  await page.evaluate(async () => {
    const finite = document.getAnimations().filter((animation) =>
      animation.effect?.getComputedTiming().iterations !== Infinity,
    );
    await Promise.allSettled(finite.map((animation) => animation.finished));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
  await expect.poll(() => page.evaluate(() => {
    const heading = document.querySelector('h1');
    if (!heading) return false;
    const probe = document.createElement('span');
    probe.style.color = 'var(--foreground)';
    document.body.append(probe);
    const foreground = getComputedStyle(probe).color;
    probe.remove();
    return getComputedStyle(heading).color === foreground;
  }), { message: 'main heading must remain readable with the current foreground token' }).toBe(true);
}
