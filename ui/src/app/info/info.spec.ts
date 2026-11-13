import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { FakeCore, provideFakeCore, settle } from '../testing/fake-core';
import { AUTHOR_NPUB, InfoPage } from './info.page';
import { LicensesPage } from './licenses.page';

describe('InfoPage', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    TestBed.configureTestingModule({ providers: [provideRouter([]), provideFakeCore(core)] });
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

  it('names the MIT License and links to the open-source licenses', async () => {
    const root = (await render()).nativeElement as HTMLElement;
    expect(root.textContent).toContain('Gooti Desktop is under the MIT License.');
    expect(root.querySelector('[data-link="licenses"]')?.getAttribute('href')).toBe('/info/licenses');
  });
});

describe('LicensesPage', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    TestBed.configureTestingModule({ providers: [provideRouter([]), provideFakeCore(core)] });
  });

  it('shows the license list from the core', async () => {
    const fixture = TestBed.createComponent(LicensesPage);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    expect(core.licensesText).toHaveBeenCalled();
    expect(root.querySelector('pre.licenses')?.textContent).toContain('Copyright (c) 2026 Sam Hayes');
  });

  it('says when the list is missing', async () => {
    core.licensesText.mockRejectedValueOnce({
      code: 'licenses',
      message: 'The license list is missing from this installation.',
    });
    const fixture = TestBed.createComponent(LicensesPage);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    expect(root.querySelector('[role="alert"]')?.textContent).toContain('The license list is missing');
    expect(root.querySelector('pre.licenses')).toBeNull();
  });
});
