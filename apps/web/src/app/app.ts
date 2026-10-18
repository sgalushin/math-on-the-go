import { Component, afterRenderEffect, computed, inject, signal, viewChild } from '@angular/core';
import type { ElementRef } from '@angular/core';
import { Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import type { Routes } from '@angular/router';
import { Auth, authGuard } from './core/auth';
import { Store, messageOf } from './core/store';

@Component({
  selector: 'math-app',
  standalone: true,
  imports: [RouterOutlet],
  template: '<router-outlet />',
})
export class App {}
@Component({
  standalone: true,
  imports: [RouterOutlet, RouterLink, RouterLinkActive],
  host: { '(document:keydown.escape)': 'closeAccount(true)' },
  template: `
    <a class="skip-link" href="#main-content">Skip to content</a>
    <div class="app-shell">
      <header class="app-header">
        <a routerLink="/" class="brand" aria-label="Math on the go home">
          <span class="brand-mark" aria-hidden="true">m<span>·</span></span>
          <span>Math <span class="brand-light">on the go</span></span>
        </a>
        <div class="user-menu">
          <button
            #accountButton
            type="button"
            class="avatar"
            aria-haspopup="menu"
            [attr.aria-expanded]="accountOpen()"
            aria-controls="account-menu"
            [attr.aria-label]="auth.email() ? 'Account: ' + auth.email() : 'Account'"
            (click)="accountOpen() ? closeAccount() : accountOpen.set(true)"
          >
            @if (initial()) {
              <span aria-hidden="true">{{ initial() }}</span>
            } @else {
              <svg viewBox="0 0 24 24" aria-hidden="true">
                <circle cx="12" cy="9" r="3.5" />
                <path d="M5.5 19a6.5 6.5 0 0 1 13 0" />
              </svg>
            }
          </button>
          @if (accountOpen()) {
            <button
              type="button"
              class="menu-dismiss"
              aria-label="Close account menu"
              (click)="closeAccount(true)"
            ></button>
            <div
              #accountMenu
              id="account-menu"
              class="menu account-menu"
              role="menu"
              aria-label="Account"
              (keydown)="menuKey($event)"
            >
              <p class="menu-label">{{ auth.email() || 'Your account' }}</p>
              <a class="menu-item" role="menuitem" routerLink="/profile" (click)="closeAccount()">
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <circle cx="12" cy="8" r="3" />
                  <path d="M5 21v-3a7 7 0 0 1 14 0v3" />
                </svg>
                Profile
              </a>
              <a
                class="menu-item"
                role="menuitem"
                routerLink="/profile/units"
                (click)="closeAccount()"
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M4 4h16v16H4zM8 12l3 3 5-6" />
                </svg>
                Studied units
              </a>
              <a
                class="menu-item"
                role="menuitem"
                routerLink="/profile"
                fragment="preferences"
                (click)="closeAccount()"
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
                  <circle cx="16" cy="7" r="2" />
                  <circle cx="10" cy="17" r="2" />
                </svg>
                Settings
              </a>
            </div>
          }
        </div>
      </header>
      <nav class="primary-nav" aria-label="Main navigation">
        @for (item of nav; track item.path) {
          <a
            [routerLink]="item.path"
            routerLinkActive="active"
            [routerLinkActiveOptions]="{ exact: item.path === '/' }"
            ariaCurrentWhenActive="page"
            ><svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="1.6"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
            >
              <path [attr.d]="item.icon" /></svg
            ><span>{{ item.label }}</span></a
          >
        }
      </nav>
      <main id="main-content" tabindex="-1">
        @if (store.catalog.isLoading() || store.settings.isLoading()) {
          <div class="global-loading" role="status">Loading your practice workspace…</div>
        }
        @if (store.catalog.error() || store.settings.error()) {
          <div class="error" role="alert">
            {{ messageOf(store.catalog.error() || store.settings.error())
            }}<button class="text-button" (click)="retry()">Retry loading</button>
          </div>
        }
        <router-outlet />
      </main>
      <footer class="app-footer">
        <span>MST124 / MST125</span><span>Practice. Recall. Repeat.</span>
      </footer>
    </div>
  `,
})
export class Shell {
  readonly store = inject(Store);
  readonly auth = inject(Auth);
  readonly accountOpen = signal(false);
  readonly initial = computed(() => this.auth.email()?.trim().charAt(0).toUpperCase() ?? '');
  private readonly accountButton = viewChild<ElementRef<HTMLButtonElement>>('accountButton');
  private readonly accountMenu = viewChild<ElementRef<HTMLElement>>('accountMenu');
  readonly messageOf = messageOf;
  readonly nav = [
    { path: '/', label: 'Today', icon: 'M3 10 12 3l9 7M5 9v12h5v-7h4v7h5V9' },
    { path: '/practice', label: 'Practice', icon: 'M5 19h14M7 15l9-10 3 3-9 10-4 1 1-4z' },
    { path: '/history', label: 'History', icon: 'M4 8a9 9 0 1 1-1 7M4 3v5h5M12 7v6l4 2' },
    { path: '/progress', label: 'Progress', icon: 'M4 20V4M4 20h17M8 16v-4M13 16V8M18 16V5' },
  ];
  constructor() {
    afterRenderEffect(() => {
      this.accountMenu()?.nativeElement.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    });
  }
  closeAccount(restoreFocus = false): void {
    if (!this.accountOpen()) return;
    this.accountOpen.set(false);
    if (restoreFocus) this.accountButton()?.nativeElement.focus();
  }
  menuKey(event: KeyboardEvent): void {
    if (event.key === 'Tab') {
      this.closeAccount(true);
      return;
    }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
    event.preventDefault();
    const items = Array.from(
      this.accountMenu()?.nativeElement.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [],
    );
    const index = items.indexOf(document.activeElement as HTMLElement);
    const next =
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? items.length - 1
          : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[next]?.focus();
  }
  retry(): void {
    if (this.store.catalog.error()) this.store.catalog.reload();
    if (this.store.settings.error()) this.store.settings.reload();
  }
}
@Component({
  standalone: true,
  imports: [RouterLink],
  template: `<main class="auth-page">
    <a class="brand" routerLink="/">math on the go</a>
    <p class="eyebrow">Your private notebook</p>
    <h1>{{ error() ? 'Let’s reconnect.' : 'Opening your notebook…' }}</h1>
    @if (error()) {
      <p class="error" role="alert">{{ error() }}</p>
      <button class="button" [disabled]="busy()" (click)="retry()">Try signing in again</button>
    } @else {
      <p role="status">Completing secure sign-in.</p>
    }
  </main>`,
})
export class AuthPage {
  readonly auth = inject(Auth);
  readonly router = inject(Router);
  readonly busy = signal(false);
  readonly error = signal('');
  constructor() {
    void this.complete();
  }
  private async complete(): Promise<void> {
    try {
      if (location.pathname === '/auth/callback') {
        await this.auth.callback();
        await this.router.navigateByUrl('/');
      } else {
        await this.auth.ready();
        if (!this.auth.enabled()) await this.router.navigateByUrl('/');
        else this.error.set('Sign in to open your private notebook.');
      }
    } catch (error) {
      this.error.set(this.auth.error() || messageOf(error));
    }
  }
  async retry(): Promise<void> {
    this.busy.set(true);
    this.error.set('');
    try {
      await this.auth.ready();
      if (this.auth.enabled()) await this.auth.login();
      else await this.router.navigateByUrl('/');
    } catch (error) {
      this.error.set(messageOf(error));
    } finally {
      this.busy.set(false);
    }
  }
}
@Component({
  standalone: true,
  imports: [RouterLink],
  template: `<section class="empty">
    <p class="eyebrow">Page not found</p>
    <h1>This page is not here</h1>
    <p>Open your profile for preferences and studied history, or start a practice session.</p>
    <div class="form-actions">
      <a class="button" routerLink="/practice">Practice</a>
      <a class="button secondary" routerLink="/profile">Profile</a>
    </div>
  </section>`,
})
export class NotFoundPage {}
export const routes: Routes = [
  { path: 'auth/callback', component: AuthPage },
  { path: 'auth', component: AuthPage },
  {
    path: '',
    component: Shell,
    canActivate: [authGuard],
    children: [
      {
        path: '',
        title: 'Today · Math on the go',
        loadComponent: () => import('./pages/home').then((m) => m.HomePage),
      },
      {
        path: 'profile/units',
        title: 'Studied units · Math on the go',
        loadComponent: () => import('./pages/studied-units').then((m) => m.StudiedUnitsPage),
      },
      {
        path: 'practice',
        title: 'Practice · Math on the go',
        loadComponent: () => import('./pages/practice').then((m) => m.PracticePage),
      },
      {
        path: 'history',
        title: 'History · Math on the go',
        loadComponent: () => import('./pages/history').then((m) => m.HistoryPage),
      },
      {
        path: 'history/:id',
        title: 'Attempt · Math on the go',
        loadComponent: () => import('./pages/attempt-detail').then((m) => m.AttemptDetailPage),
      },
      {
        path: 'progress',
        title: 'Progress · Math on the go',
        loadComponent: () => import('./pages/progress').then((m) => m.ProgressPage),
      },
      {
        path: 'profile',
        title: 'Profile · Math on the go',
        loadComponent: () => import('./pages/profile').then((m) => m.ProfilePage),
      },
      { path: '**', title: 'Not found · Math on the go', component: NotFoundPage },
    ],
  },
];
