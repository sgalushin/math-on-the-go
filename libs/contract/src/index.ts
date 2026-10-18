export type ModuleId = 'MST124' | 'MST125';
export type Kind = 'flashcard' | 'task';
export type Difficulty = 'quick' | 'standard' | 'stretch';
export type AnswerFormat = 'text' | 'choice' | 'self';
export type Outcome =
  'unfinished' | 'independent' | 'assisted' | 'incorrect' | 'self_assessed' | 'skipped';
export type Assessment = 'automatic' | 'self';
export type PracticeMode = 'review' | 'focused' | 'suggested';
export interface Topic {
  id: string;
  title: string;
  prerequisites: string[];
}
export interface Unit {
  id: string;
  module: ModuleId;
  number: number;
  title: string;
  sourcePage: number;
  topics: Topic[];
}
export interface Catalog {
  units: Unit[];
}
export interface Source {
  handbook: string;
  page: number;
  section: string;
}
export interface Graph {
  xMin: number;
  xMax: number;
  yMin: number;
  yMax: number;
  series: { label: string; points: { x: number; y: number }[] }[];
}
export interface Verification {
  method: 'handbook' | 'deterministic' | 'independent-model' | 'self-assessed';
  models: string[];
  checkedAt: number | null;
}
export interface Question {
  id: string;
  topicId: string;
  kind: Kind;
  difficulty: Difficulty;
  prompt: string;
  answer: string;
  acceptedAnswers: string[];
  choices: string[];
  hints: string[];
  explanation: string;
  answerFormat: AnswerFormat;
  source: Source;
  verification: Verification;
  graph?: Graph;
  flagged: boolean;
  template?: { type: string; parameters: Record<string, number> };
}
export interface Submission {
  answer: string;
  at: number;
  correct: boolean | null;
}
export interface Attempt {
  id: string;
  question: Question;
  startedAt: number;
  updatedAt: number;
  completedAt: number | null;
  submissions: Submission[];
  hintsUsed: number;
  solutionRevealed: boolean;
  outcome: Outcome;
  assessment: Assessment;
  rating: 'again' | 'good' | null;
  reported: boolean;
}
export interface Settings {
  studiedUnitIds: string[];
  defaultDifficulty: 'any' | Difficulty;
  sessionLength: number;
}
export interface PracticeSelection {
  mode: PracticeMode;
  kind: 'mixed' | 'task' | 'formula';
  unitIds: string[];
  topicIds: string[];
  difficulty: 'any' | Difficulty;
}
export interface CreateSession extends PracticeSelection {
  requestId: string;
  length: number;
}
export interface PracticeSession {
  id: string;
  status: 'generating' | 'ready' | 'failed' | 'finished';
  request: PracticeSelection;
  length: number;
  generated: number;
  attemptIds: string[];
  error: string | null;
  createdAt: number;
  updatedAt: number;
}
export interface SessionNextRequest {
  eventId: string;
  afterAttemptId: string | null;
}
export interface AttemptAction {
  action: 'hint' | 'reveal' | 'answer' | 'grade' | 'skip';
  answer?: string;
  rating?: 'again' | 'good';
  selfCorrect?: boolean;
  eventId: string;
}
export interface DifficultyProgress {
  attempts: number;
  independent: number;
  assisted: number;
  incorrect: number;
}
export interface TopicProgress {
  topicId: string;
  attempts: number;
  independent: number;
  assisted: number;
  incorrect: number;
  selfAssessed: number;
  skipped: number;
  unfinished: number;
  flagged: number;
  recentAccuracy: number | null;
  previousAccuracy: number | null;
  lastPractised: number | null;
  byDifficulty: Record<Difficulty, DifficultyProgress>;
}
export interface Progress {
  totalAttempts: number;
  completedTasks: number;
  independent: number;
  assisted: number;
  incorrect: number;
  studiedUnits: number;
  coveredTopics: number;
  totalTopics: number;
  dueFlashcards: number;
  newFlashcards: number;
  reviewedFlashcards: number;
  topics: TopicProgress[];
  recentDays: { date: string; tasks: number; flashcards: number }[];
}
export interface HistoryResponse {
  attempts: Attempt[];
  nextCursor: string | null;
}
export interface HistoryFilters {
  topicId?: string;
  unitId?: string;
  kind?: Kind;
  outcome?: Outcome;
  difficulty?: Difficulty;
  from?: number;
  to?: number;
  cursor?: string;
  limit?: number;
}
