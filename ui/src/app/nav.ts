export interface NavItem {
  path: string;
  label: string;
  /** Bootstrap icon name without the `bi-` prefix. */
  icon: string;
}

/** Sidebar sections, top to bottom. */
export const NAV_ITEMS: readonly NavItem[] = [
  { path: 'identities', label: 'Identities', icon: 'person-badge' },
  { path: 'connections', label: 'Connections', icon: 'plug' },
  { path: 'activity', label: 'Activities', icon: 'clock-history' },
  { path: 'backup', label: 'Backup', icon: 'arrow-repeat' },
  { path: 'settings', label: 'Settings', icon: 'gear' },
  { path: 'info', label: 'Info', icon: 'info-circle' },
];
