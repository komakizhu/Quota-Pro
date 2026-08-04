export interface OrbDragState {
  suppressNextClick: boolean;
}

export const createOrbDragState = (): OrbDragState => ({ suppressNextClick: false });

export const recordOrbDrag = (state: OrbDragState): OrbDragState => ({
  ...state,
  suppressNextClick: true,
});

export const consumeOrbClick = (state: OrbDragState): { state: OrbDragState; suppressed: boolean } => ({
  state: { suppressNextClick: false },
  suppressed: state.suppressNextClick,
});
