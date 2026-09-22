/**
 * The first-run guidance is dismissed per instance and remembered in the
 * browser that dismissed it. The instance-wide `onboarding_complete` branding
 * switch keeps its own meaning — "onboarding is finished here" — so an operator
 * who only wants the banner out of the way does not have to answer for every
 * other principal, and can ask for it back without touching the branding record.
 */
export const ONBOARDING_DISMISSED_KEY = 'pangolin-onboarding-dismissed'
/** Fired whenever the dismissal changes, so an open shell re-reads it. */
export const ONBOARDING_CHANGED_EVENT = 'pangolin-onboarding-changed'

/** The instance the dismissal belongs to; a different instance shows it again. */
export function isOnboardingDismissed(instance: string): boolean {
  try {
    return localStorage.getItem(ONBOARDING_DISMISSED_KEY) === instance
  } catch {
    // A browser that refuses storage has no way to remember the choice, so the
    // guidance stays rather than being dismissed on every render.
    return false
  }
}

export function dismissOnboarding(instance: string) {
  try {
    localStorage.setItem(ONBOARDING_DISMISSED_KEY, instance)
  } catch {
    return
  }
  announceOnboardingChange()
}

export function restoreOnboarding() {
  try {
    localStorage.removeItem(ONBOARDING_DISMISSED_KEY)
  } catch {
    return
  }
  announceOnboardingChange()
}

function announceOnboardingChange() {
  window.dispatchEvent(new Event(ONBOARDING_CHANGED_EVENT))
}
