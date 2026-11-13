import { TestBed } from '@angular/core/testing';
import { FakeCore, provideFakeCore, settle } from '../testing/fake-core';
import { AUTHOR_NPUB, InfoPage } from './info.page';

describe('InfoPage', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    TestBed.configureTestingModule({ providers: [provideFakeCore(core)] });
  });

  const render = async () => {
    const fixture = TestBed.createComponent(InfoPage);
    await settle(fixture);
    return fixture;
  };

  it('shows the version, the channel and the author key', async () => {
    core.appInfo.set({ version: '0.4.2', channel: 'alpha' });
    const root = (await render()).nativeElement as HTMLElement;
    expect(root.querySelector('[data-info="version"]')?.textContent?.trim()).toBe('0.4.2 (alpha)');
    expect(root.querySelector<HTMLInputElement>('.sam-field input')?.value).toBe(AUTHOR_NPUB);
  });

  it('opens each link through the core', async () => {
    const root = (await render()).nativeElement as HTMLElement;
    for (const link of ['website', 'source', 'author'] as const) {
      root.querySelector<HTMLButtonElement>(`[data-link="${link}"]`)!.click();
      expect(core.openLink).toHaveBeenLastCalledWith(link);
    }
  });

  it('says when the browser could not be opened', async () => {
    core.openLink.mockRejectedValueOnce({ code: 'open', message: 'Could not open the browser: no handler' });
    const fixture = await render();
    const root = fixture.nativeElement as HTMLElement;
    root.querySelector<HTMLButtonElement>('[data-link="website"]')!.click();
    await settle(fixture);
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('Could not open the browser');
  });
});
