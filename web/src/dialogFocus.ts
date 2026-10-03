/** Restore only a connected opener after immediate sensitive-dialog teardown. */
export function restoreConnectedOpener(opener: HTMLElement | null, sameContext: () => boolean = () => true) {
  queueMicrotask(() => {
    if (sameContext() && opener?.isConnected && document.activeElement === document.body)
      opener.focus({ preventScroll: true })
  })
}
