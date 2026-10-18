import { Injectable, inject, signal } from '@angular/core';
import { HttpErrorResponse } from '@angular/common/http';
import type { HttpInterceptorFn } from '@angular/common/http';
import { Router } from '@angular/router';
import type { CanActivateFn } from '@angular/router';
import { UserManager, WebStorageStateStore } from 'oidc-client-ts';
import { catchError, from, switchMap, throwError } from 'rxjs';

interface AuthConfig {
  authority: string;
  clientId: string;
  domain: string;
}
@Injectable({ providedIn: 'root' })
export class Auth {
  readonly enabled = signal(false);
  readonly email = signal<string | null>(null);
  readonly error = signal('');
  private manager: UserManager | null = null;
  private config: AuthConfig | null = null;
  private loading: Promise<void> | null = null;
  ready(): Promise<void> {
    return (this.loading ??= this.load().catch((error) => {
      this.loading = null;
      this.error.set('Unable to load account configuration. Check your connection and retry.');
      throw error;
    }));
  }
  async login(): Promise<void> {
    await this.ready();
    await this.manager?.signinRedirect();
  }
  async callback(): Promise<void> {
    await this.ready();
    const user = await this.manager?.signinRedirectCallback();
    this.email.set(user?.profile.email ?? null);
  }
  async token(): Promise<string | null> {
    await this.ready();
    let user = await this.manager?.getUser();
    if (user?.expired) {
      try {
        user = await this.manager?.signinSilent();
      } catch {
        return null;
      }
    }
    return user?.access_token ?? null;
  }
  async signOut(): Promise<void> {
    await this.ready();
    await this.manager?.removeUser();
    if (!this.config) return;
    const url = new URL('/logout', this.config.domain);
    url.searchParams.set('client_id', this.config.clientId);
    url.searchParams.set('logout_uri', `${location.origin}/`);
    location.assign(url.toString());
  }
  private async load(): Promise<void> {
    const response = await fetch('/config.json', { cache: 'no-store' });
    if (!response.ok) throw new Error('Runtime configuration unavailable');
    const config = (await response.json()) as { auth: AuthConfig | null };
    this.config = config.auth;
    this.enabled.set(!!config.auth);
    if (!config.auth) return;
    this.manager = new UserManager({
      authority: config.auth.authority,
      client_id: config.auth.clientId,
      redirect_uri: `${location.origin}/auth/callback`,
      post_logout_redirect_uri: `${location.origin}/`,
      response_type: 'code',
      scope: 'openid email profile',
      automaticSilentRenew: true,
      userStore: new WebStorageStateStore({ store: localStorage }),
    });
    this.manager.events.addUserLoaded((user) => this.email.set(user.profile.email ?? null));
    this.manager.events.addUserUnloaded(() => this.email.set(null));
    const user = await this.manager.getUser();
    this.email.set(user?.profile.email ?? null);
    this.error.set('');
  }
}
export const authGuard: CanActivateFn = async () => {
  const auth = inject(Auth);
  const router = inject(Router);
  try {
    await auth.ready();
    if (!auth.enabled() || (await auth.token())) return true;
    await auth.login();
    return false;
  } catch {
    return router.createUrlTree(['/auth']);
  }
};
export const authInterceptor: HttpInterceptorFn = (request, next) => {
  if (!request.url.startsWith('/api')) return next(request);
  const auth = inject(Auth);
  return from(auth.token()).pipe(
    switchMap((token) =>
      next(token ? request.clone({ setHeaders: { Authorization: `Bearer ${token}` } }) : request),
    ),
    catchError((error: unknown) => {
      if (error instanceof HttpErrorResponse && error.status === 401 && auth.enabled())
        void auth.login();
      return throwError(() => error);
    }),
  );
};
