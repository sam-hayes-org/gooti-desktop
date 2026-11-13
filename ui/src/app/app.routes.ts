import { Routes } from '@angular/router';
import { IdentitiesPage } from './identities/identities.page';
import { IdentityDetailPage } from './identities/identity-detail.page';
import { IdentityNewPage } from './identities/identity-new.page';
import { ActivityPage } from './activity/activity.page';
import { ConnectionPage } from './connections/connection.page';
import { ConnectionsPage } from './connections/connections.page';
import { ConnectionNewPage } from './connections/connection-new.page';
import { SettingsPage } from './settings/settings.page';
import { BackupPage } from './backup/backup.page';
import { InfoPage } from './info/info.page';

/** Pages inside the unlocked shell. The vault screens are not routes. */
export const routes: Routes = [
  { path: '', pathMatch: 'full', redirectTo: 'identities' },
  { path: 'identities', component: IdentitiesPage, title: 'Identities · Gooti' },
  { path: 'identities/new', component: IdentityNewPage, title: 'New identity · Gooti' },
  { path: 'identities/:id', component: IdentityDetailPage, title: 'Identity · Gooti' },
  { path: 'backup', component: BackupPage, title: 'Backup · Gooti' },
  { path: 'connections', component: ConnectionsPage, title: 'Connections · Gooti' },
  { path: 'connections/new', component: ConnectionNewPage, title: 'Connect new app · Gooti' },
  { path: 'connections/:id', component: ConnectionPage, title: 'App · Gooti' },
  { path: 'activity', component: ActivityPage, title: 'Activities · Gooti' },
  { path: 'settings', component: SettingsPage, title: 'Settings · Gooti' },
  { path: 'info', component: InfoPage, title: 'Info · Gooti' },
  { path: '**', redirectTo: 'identities' },
];
