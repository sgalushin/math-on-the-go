import { Component, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { Store } from '../core/store';

@Component({
  standalone: true,
  imports: [RouterLink],
  template: `
    <a routerLink="/profile" class="back-link">← Profile</a>
    <header class="page-heading">
      <p class="eyebrow">Your learning history</p>
      <h1>Studied units</h1>
      <p class="lede">
        Mark what you have covered. Choose what to practise separately, when you start a session.
      </p>
    </header>
    <div class="segmented" role="group" aria-label="Module">
      @for (name of modules; track name) {
        <button
          type="button"
          [class.active]="module() === name"
          [attr.aria-pressed]="module() === name"
          (click)="module.set(name)"
        >
          {{ name }}
        </button>
      }
    </div>
    @if (module() === 'MST125') {
      <p class="small muted">
        The MST125 handbook covers Units 3–12. Units 1–2 are not separately represented.
      </p>
    }
    @if (store.settingsError()) {
      <p class="error" role="alert">{{ store.settingsError() }}</p>
    }
    @if (store.settings.hasValue()) {
      @let settings = store.settings.value();
      <div class="selection-summary">
        <span>{{ settings.studiedUnitIds.length }} units marked studied</span>
        <a routerLink="/practice" class="text-link">Start a session →</a>
      </div>
      <p class="save-state" role="status" aria-live="polite">
        {{
          store.saving()
            ? 'Saving studied history…'
            : saved()
              ? 'Studied history saved.'
              : 'Changes save automatically.'
        }}
      </p>
      <div class="unit-list">
        @for (unit of visible(); track unit.id) {
          <article class="unit-card" [class.studied]="settings.studiedUnitIds.includes(unit.id)">
            <div class="unit-heading">
              <span class="unit-number">{{ unit.number < 10 ? '0' : '' }}{{ unit.number }}</span>
              <div>
                <p class="eyebrow">{{ unit.module }}</p>
                <h2>{{ unit.title }}</h2>
              </div>
            </div>
            <label class="check-label">
              <input
                type="checkbox"
                [checked]="settings.studiedUnitIds.includes(unit.id)"
                [disabled]="store.saving()"
                (change)="toggle(unit.id)"
              />
              <span>Studied <small>I have covered this unit</small></span>
            </label>
            <details>
              <summary>
                Topics & prerequisites <span>{{ unit.topics.length }} topics</span>
              </summary>
              <p class="small muted">
                Practice includes a topic only when its unit and all its prerequisites are studied.
              </p>
              @for (topic of unit.topics; track topic.id) {
                <div class="topic-reference">
                  <strong>{{ topic.title }}</strong
                  ><small>{{ prerequisites(topic.prerequisites) }}</small>
                </div>
              }
            </details>
          </article>
        }
      </div>
    }
  `,
})
export class StudiedUnitsPage {
  readonly store = inject(Store);
  readonly modules = ['MST124', 'MST125'];
  readonly module = signal('MST124');
  readonly saved = signal(false);
  readonly visible = computed(() =>
    this.store.units().filter((unit) => unit.module === this.module()),
  );
  async toggle(id: string): Promise<void> {
    if (!this.store.settings.hasValue()) return;
    const current = this.store.settings.value().studiedUnitIds;
    this.saved.set(false);
    this.saved.set(
      await this.store.saveSettings({
        studiedUnitIds: current.includes(id)
          ? current.filter((value) => value !== id)
          : [...current, id],
      }),
    );
  }
  prerequisites(ids: string[]): string {
    const studied = this.store.settings.hasValue()
      ? this.store.settings.value().studiedUnitIds
      : [];
    return ids.length
      ? 'Prerequisites: ' +
          ids
            .map((id) => {
              const unit = this.store.units().find((value) => value.id === id);
              return `${unit ? unit.module + ' Unit ' + unit.number : id} (${studied.includes(id) ? 'studied' : 'not studied'})`;
            })
            .join(', ')
      : 'No additional unit prerequisites';
  }
}
