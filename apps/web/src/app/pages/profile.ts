import { Component, effect, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import type { Settings } from '@math/contract';
import { Store, messageOf } from '../core/store';
import { Auth } from '../core/auth';

@Component({
  standalone: true,
  imports: [FormsModule, RouterLink],
  template: `
    <header class="page-heading">
      <p class="eyebrow">Your account</p>
      <h1>Profile</h1>
      <p class="lede">Your studied history, practice defaults and account.</p>
    </header>
    @if (store.settings.hasValue()) {
      <a routerLink="/profile/units" class="profile-unit-link">
        <div>
          <p class="eyebrow">Learning history</p>
          <h2>Studied units</h2>
          <p>
            {{ store.settings.value().studiedUnitIds.length }} units covered. Manage what you have
            studied here; session choices stay in Practice.
          </p>
        </div>
        <span aria-hidden="true">→</span>
      </a>
      <section class="panel" id="preferences">
        <h2>Practice defaults</h2>
        <form (ngSubmit)="save()">
          <fieldset [disabled]="store.saving()">
            <label for="default-length">Default session length</label
            ><input
              id="default-length"
              name="sessionLength"
              type="number"
              min="1"
              max="20"
              [(ngModel)]="sessionLength"
              required
            />
            <p class="field-help">
              Between 1 and 20 questions. Individual sessions can be shorter.
            </p>
            <label for="default-difficulty">Default difficulty</label
            ><select id="default-difficulty" name="difficulty" [(ngModel)]="defaultDifficulty">
              <option value="any">All difficulties</option>
              <option value="quick">Quick</option>
              <option value="standard">Standard</option>
              <option value="stretch">Stretch</option>
            </select>
            <button class="button" type="submit" [disabled]="!changed || !valid || store.saving()">
              {{ store.saving() ? 'Saving…' : 'Save preferences' }}
            </button>
          </fieldset>
        </form>
        @if (store.settingsError()) {
          <p class="error" role="alert">{{ store.settingsError() }}</p>
        }
        @if (saved()) {
          <p role="status" class="success">Preferences saved.</p>
        }
      </section>
    }
    <section class="panel account-panel">
      <div>
        <p class="eyebrow">Your account</p>
        <h2>{{ auth.enabled() ? 'Signed in' : 'Local development' }}</h2>
        <p>
          {{
            auth.email() ||
              (auth.enabled()
                ? 'Signed in with Cognito'
                : 'Authentication is disabled by local runtime configuration.')
          }}
        </p>
      </div>
      @if (auth.enabled()) {
        <button class="button secondary" (click)="signOut()" [disabled]="signingOut()">
          {{ signingOut() ? 'Signing out…' : 'Sign out' }}
        </button>
      }
      @if (error()) {
        <p class="error" role="alert">{{ error() }}</p>
      }
    </section>
    <p class="quiet-note">
      This is an online app. Install it from your browser for a home-screen shortcut; practice and
      history still require a connection. No offline data cache is kept.
    </p>
  `,
})
export class ProfilePage {
  readonly store = inject(Store);
  readonly auth = inject(Auth);
  readonly error = signal('');
  readonly saved = signal(false);
  readonly signingOut = signal(false);
  sessionLength = 5;
  defaultDifficulty: Settings['defaultDifficulty'] = 'any';
  private initialized = false;
  get changed(): boolean {
    if (!this.store.settings.hasValue()) return false;
    const value = this.store.settings.value();
    return (
      this.sessionLength !== value.sessionLength ||
      this.defaultDifficulty !== value.defaultDifficulty
    );
  }
  get valid(): boolean {
    return (
      Number.isInteger(this.sessionLength) && this.sessionLength >= 1 && this.sessionLength <= 20
    );
  }
  constructor() {
    effect(() => {
      if (this.store.settings.hasValue() && !this.initialized) {
        const s = this.store.settings.value();
        this.sessionLength = s.sessionLength;
        this.defaultDifficulty = s.defaultDifficulty;
        this.initialized = true;
      }
    });
  }
  async save(): Promise<void> {
    if (!this.valid) return;
    this.saved.set(false);
    this.saved.set(
      await this.store.saveSettings({
        sessionLength: Number(this.sessionLength),
        defaultDifficulty: this.defaultDifficulty,
      }),
    );
  }
  async signOut(): Promise<void> {
    this.signingOut.set(true);
    try {
      await this.auth.signOut();
    } catch (error) {
      this.error.set(messageOf(error));
      this.signingOut.set(false);
    }
  }
}
