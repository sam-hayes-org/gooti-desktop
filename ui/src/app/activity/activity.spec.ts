import { ActivityEntry } from '../core/core.service';
import { TestBed } from '@angular/core/testing';
import { ConfirmService } from '../shared/confirm.service';
import { ALICE, FakeCore, button, provideFakeCore, settle, text } from '../testing/fake-core';
import { ActivityPage } from './activity.page';

const LOG: ActivityEntry[] = [
  {
    at: 3,
    type: 'request',
    app: 'Coracle',
    connectionId: 'c1',
    identityPubkey: ALICE.pubkey,
    method: 'sign_event',
    kind: 1,
    approved: true,
    reason: 'remembered',
  },
  {
    at: 2,
    type: 'request',
    app: 'Gooti browsers',
    connectionId: 'c2',
    identityPubkey: ALICE.pubkey,
    method: 'nip44_decrypt',
    approved: false,
    reason: 'timedOut',
    site: 'primal.net',
  },
  { at: 1, type: 'connected', app: 'Coracle', connectionId: 'c1', identityPubkey: ALICE.pubkey },
  {
    at: 0,
    type: 'backup',
    automatic: true,
    result: 'updated',
    took: ['Added identity Bob'],
    sent: [],
  },
];

describe('ActivityPage', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    core.identities.set([ALICE]);
    TestBed.configureTestingModule({ providers: [provideFakeCore(core)] });
  });

  it('lists requests, connections and backups, and clears them after the confirmation', async () => {
    core.activityLog = [...LOG];
    const fixture = TestBed.createComponent(ActivityPage);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    const rows = Array.from(root.querySelectorAll<HTMLElement>('.entry')).map((row) => text(row));
    expect(rows[0]).toContain('Coracle: Sign: Note (kind 1)');
    expect(rows[0]).toContain('Approved by a remembered choice, as Alice.');
    expect(rows[1]).toContain('primal.net through Gooti browsers');
    expect(rows[1]).toContain('Nobody answered in time, as Alice.');
    expect(rows[2]).toContain('Coracle connected');
    expect(rows[3]).toContain('Automatic backup: Updated');
    expect(rows[3]).toContain('On this desktop: Added identity Bob');

    const confirm = TestBed.inject(ConfirmService);
    button(root, 'Clear activities').click();
    expect(confirm.pending()?.message).toBe('Clear activities?');
    confirm.answer(true);
    await settle(fixture);
    expect(core.clearActivity).toHaveBeenCalledOnce();
    expect(text(root)).toContain('No activity yet');
  });

  it('shows one app, or backups, when filtered', async () => {
    core.activityLog = [...LOG];
    const fixture = TestBed.createComponent(ActivityPage);
    fixture.componentRef.setInput('app', 'c1');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    const rows = () =>
      Array.from(root.querySelectorAll<HTMLElement>('.entry')).map((row) => text(row));
    expect(rows()).toHaveLength(2);
    expect(rows()[0]).toContain('Coracle: Sign');
    expect(rows()[1]).toContain('Coracle connected');
    expect(root.textContent).not.toContain('Clear activities');

    fixture.componentRef.setInput('app', 'backups');
    await settle(fixture);
    expect(rows()).toHaveLength(1);
    expect(rows()[0]).toContain('Automatic backup');

    fixture.componentRef.setInput('app', 'gone');
    await settle(fixture);
    expect(text(root)).toContain('Nothing here yet');
  });
});
