import { Component, computed, effect, input, output, signal } from '@angular/core';
import { DatePipe } from '@angular/common';
import { FormsModule } from '@angular/forms';
import type { Attempt, AttemptAction } from '@math/contract';
import { MathGraph, MathText } from './math';

@Component({
  selector: 'attempt-view',
  standalone: true,
  imports: [FormsModule, DatePipe, MathText, MathGraph],
  template: `
    @let a = attempt(); @let q = a.question;
    <article class="question-card">
      <div class="question-meta">
        <span class="tag">{{ q.kind === 'flashcard' ? 'Recall card' : 'Practice task' }}</span
        ><span>{{ q.difficulty }}</span
        ><span>{{ a.assessment === 'self' ? 'Self-assessed' : 'Automatically checked' }}</span>
      </div>
      <div class="question-prompt"><math-text [text]="q.prompt" /></div>
      @if (q.graph; as graph) {
        <math-graph [graph]="graph" />
      }
      @if (interactive() && a.outcome === 'unfinished' && q.kind === 'task') {
        <form class="answer-form" (ngSubmit)="submit()">
          @if (q.answerFormat === 'choice') {
            <fieldset>
              <legend>Your answer</legend>
              @for (choice of q.choices; track $index) {
                <label class="answer-choice" [class.chosen]="answer() === choice"
                  ><input
                    type="radio"
                    name="answer"
                    [value]="choice"
                    [ngModel]="answer()"
                    (ngModelChange)="answer.set($event)"
                    [disabled]="busy()" /><math-text [text]="choice"
                /></label>
              }
            </fieldset>
          } @else {
            <label for="task-answer">{{
              q.answerFormat === 'self' ? 'Your reasoning' : 'Your answer'
            }}</label
            ><textarea
              id="task-answer"
              name="answer"
              rows="3"
              autocomplete="off"
              autocapitalize="off"
              spellcheck="false"
              [ngModel]="answer()"
              (ngModelChange)="answer.set($event)"
              [disabled]="busy() || (q.answerFormat === 'self' && a.submissions.length > 0)"
              [placeholder]="
                q.answerFormat === 'self'
                  ? 'Put your reasoning into words before comparing…'
                  : 'Type a number or mathematical expression…'
              "
            ></textarea>
            <p class="field-help">
              {{
                q.answerFormat === 'self'
                  ? 'This is a conceptual task: you will assess your reasoning against the explanation.'
                  : 'Enter a numerical answer: fractions (3/4), powers (2^3), sqrt(2), and pi are supported. Expressions containing variables are not automatically checked.'
              }}
            </p>
          }
          <button
            class="button"
            type="submit"
            [disabled]="
              busy() || !answer().trim() || (q.answerFormat === 'self' && a.submissions.length > 0)
            "
          >
            {{
              q.answerFormat === 'self'
                ? a.submissions.length
                  ? 'Reasoning recorded'
                  : 'Record my reasoning'
                : 'Check answer'
            }}
            <span aria-hidden="true">→</span>
          </button>
        </form>
      }
      @if (a.submissions.length) {
        <div class="submission-feedback" aria-live="polite">
          @let last = a.submissions[a.submissions.length - 1];
          <strong>{{
            last.correct === null
              ? 'Your reasoning is recorded.'
              : last.correct
                ? 'Correct.'
                : 'Not quite this time.'
          }}</strong>
          <p><math-text [text]="last.answer" /></p>
          @if (last.correct === false && !shown()) {
            <p>Reveal the explanation to work through the idea, then continue your session.</p>
          }
        </div>
      }
      @if (a.hintsUsed) {
        <section class="hints">
          <p class="eyebrow">A little direction</p>
          @for (hint of q.hints.slice(0, a.hintsUsed); track $index) {
            <div>
              <span class="hint-number">{{ $index + 1 }}</span
              ><math-text [text]="hint" />
            </div>
          }
        </section>
      }
      @if (interactive() && a.outcome === 'unfinished' && !a.solutionRevealed) {
        <div class="question-tools">
          @if (q.kind === 'task' && a.hintsUsed < q.hints.length) {
            <button
              class="button secondary"
              (click)="action.emit({ action: 'hint' })"
              [disabled]="busy()"
            >
              {{ a.hintsUsed ? 'Next hint' : 'A hint, please' }}
              <small>{{ a.hintsUsed }}/{{ q.hints.length }}</small>
            </button>
          }
          <button
            class="button secondary"
            (click)="action.emit({ action: 'reveal' })"
            [disabled]="
              busy() || (q.kind === 'task' && q.answerFormat === 'self' && !a.submissions.length)
            "
          >
            {{ q.kind === 'flashcard' ? 'Reveal answer' : 'Reveal worked solution' }}
          </button>
        </div>
        @if (q.kind === 'task') {
          <p class="small muted">
            Hints and revealing a solution are recorded as assistance. Self-assessed reasoning is
            tracked separately.
          </p>
        }
      }
      @if (!interactive() && !shown()) {
        <button class="button secondary" (click)="historyReveal.set(true)">
          Show answer & explanation snapshot
        </button>
      }
      @if (interactive() && a.outcome !== 'unfinished' && !a.solutionRevealed) {
        <button
          class="button secondary"
          (click)="action.emit({ action: 'reveal' })"
          [disabled]="busy()"
        >
          Show worked explanation
        </button>
      }
      @if (shown()) {
        <section class="solution">
          <p class="eyebrow">
            {{ q.kind === 'flashcard' ? 'The idea to remember' : 'Worked explanation' }}
          </p>
          <div class="solution-answer"><math-text [text]="q.answer" /></div>
          <math-text [text]="q.explanation" />
        </section>
      }
      @if (interactive() && a.outcome === 'unfinished' && a.solutionRevealed) {
        @if (q.kind === 'flashcard') {
          <p class="small">Did you recall the idea before revealing it?</p>
          <div class="grade-grid">
            <button
              class="button secondary"
              (click)="action.emit({ action: 'grade', rating: 'again' })"
              [disabled]="busy()"
            >
              Not yet <span>Review again</span></button
            ><button
              class="button"
              (click)="action.emit({ action: 'grade', rating: 'good' })"
              [disabled]="busy()"
            >
              Remembered <span>Schedule next review</span>
            </button>
          </div>
        } @else if (q.answerFormat === 'self') {
          <p class="small">
            Compare your recorded reasoning with the explanation. This is your judgement, not an
            automatic mark.
          </p>
          <div class="grade-grid">
            <button
              class="button secondary"
              (click)="action.emit({ action: 'grade', selfCorrect: false })"
              [disabled]="busy()"
            >
              Needs more work</button
            ><button
              class="button"
              (click)="action.emit({ action: 'grade', selfCorrect: true })"
              [disabled]="busy()"
            >
              My reasoning holds
            </button>
          </div>
        }
      }
      @if (a.outcome !== 'unfinished') {
        <div class="outcome-banner" role="status">
          <span class="eyebrow">Recorded outcome</span><strong>{{ outcomeLabel() }}</strong>
          @if (a.reported || q.flagged) {
            <p>Excluded from progress.</p>
          }
        </div>
      }
      <details class="source-details">
        <summary>Reference & verification</summary>
        <p>{{ q.source.handbook }}, page {{ q.source.page }} · {{ q.source.section }}</p>
        <p>
          Original practice question, aligned to the handbook topic. The source handbook is not
          reproduced.
        </p>
        <p>
          Verification: {{ q.verification.method }}
          @if (q.verification.models.length) {
            · {{ q.verification.models.join(' / ') }}
          }
          @if (q.verification.checkedAt) {
            · {{ q.verification.checkedAt | date: 'medium' }}
          }
        </p>
      </details>
    </article>
  `,
})
export class AttemptView {
  readonly attempt = input.required<Attempt>();
  readonly interactive = input(false);
  readonly busy = input(false);
  readonly action = output<Omit<AttemptAction, 'eventId'>>();
  readonly answer = signal('');
  readonly historyReveal = signal(false);
  readonly shown = computed(() => this.attempt().solutionRevealed || this.historyReveal());
  readonly outcomeLabel = computed(
    () =>
      ({
        unfinished: 'In progress',
        independent: 'Completed independently',
        assisted: 'Completed with assistance',
        incorrect: 'Not correct yet',
        self_assessed: 'Self-assessed reasoning',
        skipped: 'Skipped',
      })[this.attempt().outcome],
  );
  constructor() {
    effect(() => {
      const attempt = this.attempt();
      if (attempt.id !== this.lastId) {
        this.lastId = attempt.id;
        this.answer.set(attempt.submissions.at(-1)?.answer ?? '');
        this.historyReveal.set(false);
      }
    });
  }
  private lastId = '';
  submit(): void {
    if (this.answer().trim()) this.action.emit({ action: 'answer', answer: this.answer().trim() });
  }
}
