import { Component, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import { DatePipe } from '@angular/common';
import { Store, messageOf } from '../core/store';

@Component({
  standalone: true,
  imports: [RouterLink, DatePipe],
  template: `
    <header class="page-heading">
      <p class="eyebrow">MST124 & MST125</p>
      <h1>Ready for a question?</h1>
      <p class="lede">Practise tasks, recall formulas, or revisit your scheduled flashcards.</p>
    </header>
    @if (store.sessionId()) {
      <section class="notice resume">
        <div>
          <strong>Your session is here.</strong>
          <p>
            @if (store.session(); as session) {
              @if (session.status === 'generating') {
                Preparing tasks · {{ session.generated }} / {{ session.length }} questions ready.
              } @else if (session.status === 'failed') {
                Preparation needs your attention.
              } @else {
                {{ store.completed() }} of {{ session.length }} completed.
              }
            } @else {
              Pick up where you left off.
            }
          </p>
        </div>
        <a class="button" routerLink="/practice"
          >Resume session <span aria-hidden="true">↗</span></a
        >
      </section>
    }
    <div class="start-grid">
      <a class="start-card formula-card" routerLink="/practice" [queryParams]="{ kind: 'formula' }">
        <span class="card-index">01 / FORMULAS</span>
        <h2>Formula flashcards<span aria-hidden="true">↗</span></h2>
        <p>Fixed formulas, identities and rules from your MST124 and MST125 handbooks.</p>
        <span class="card-foot">Opens immediately · no AI generation</span>
      </a>
      <a class="start-card review-card" routerLink="/practice" [queryParams]="{ mode: 'review' }"
        ><span class="card-index">02 / REVIEW</span>
        <h2>Scheduled review<span aria-hidden="true">↗</span></h2>
        <p>Revisit due and new handbook cards with spaced repetition.</p>
        @if (store.progress.hasValue()) {
          <span class="card-foot"
            >{{ store.progress.value().dueFlashcards }} due ·
            {{ store.progress.value().newFlashcards }} new</span
          >
        } @else {
          <span class="card-foot">Your personal review queue</span>
        }
      </a>
      <a
        class="start-card practice-card"
        routerLink="/practice"
        [queryParams]="{ mode: 'suggested' }"
        ><span class="card-index">03 / EXPLORE</span>
        <h2>Practise tasks<span aria-hidden="true">↗</span></h2>
        <p>A full session of verified tasks, with space to work each one out.</p>
        <span class="card-foot">Focused or suggested practice</span></a
      >
    </div>
    <section class="section">
      <div class="section-heading">
        <h2>Your practice, in numbers</h2>
        <a routerLink="/progress">See progress <span aria-hidden="true">→</span></a>
      </div>
      @if (store.progress.isLoading()) {
        <p class="status" role="status">Opening your notebook…</p>
      }
      @if (store.progress.error()) {
        <div class="error" role="alert">
          {{ messageOf(store.progress.error()) }}
          <button class="text-button" (click)="store.progress.reload()">Retry</button>
        </div>
      }
      @if (store.progress.hasValue()) {
        @let p = store.progress.value();
        <div class="stat-grid">
          <div>
            <strong>{{ p.completedTasks }}</strong
            ><span>tasks completed</span>
          </div>
          <div>
            <strong>{{ p.reviewedFlashcards }}</strong
            ><span>cards reviewed</span>
          </div>
          <div>
            <strong>{{ p.studiedUnits }}</strong
            ><span>units studied</span>
          </div>
        </div>
        @if (!p.totalAttempts) {
          <div class="empty">
            <h3>Start your practice record</h3>
            @if (store.settings.hasValue() && store.settings.value().studiedUnitIds.length) {
              <p>
                Your studied history is ready. Start with everything you have studied, or choose a
                smaller scope.
              </p>
              <a routerLink="/practice" class="button">Start a session</a>
            } @else {
              <p>
                Add the units you have covered to your profile, then start a practice or review
                session.
              </p>
              <a routerLink="/profile/units" class="button">Add studied units</a>
            }
          </div>
        }
        @if (p.recentDays.length) {
          <div class="activity-strip">
            <h3>Recent activity</h3>
            @for (day of p.recentDays.slice(-7); track day.date) {
              <div>
                <span>{{ day.date | date: 'EEE d' }}</span
                ><strong>{{ day.tasks + day.flashcards }}</strong
                ><small>{{ day.tasks }} tasks · {{ day.flashcards }} cards</small>
              </div>
            }
          </div>
        }
      }
    </section>
    <aside class="quiet-note">
      <span class="tiny-mark" aria-hidden="true">∴</span>
      <p>
        Built around MST124 & MST125. Tasks measure practice; flashcards use spaced repetition. They
        tell different stories.
      </p>
    </aside>
  `,
})
export class HomePage {
  readonly store = inject(Store);
  readonly messageOf = messageOf;
  constructor() {
    if (!this.store.progress.isLoading()) this.store.progress.reload();
  }
}
