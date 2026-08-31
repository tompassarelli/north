
const WORK__OWNERSHIP__VERSION = "work-ownership-v1";
export { WORK__OWNERSHIP__VERSION as "WORK_OWNERSHIP_VERSION" };

const WORK__OWNERSHIP__SCHEMA__ID = "urn:agent-machinery:schema:work-ownership:v1";
export { WORK__OWNERSHIP__SCHEMA__ID as "WORK_OWNERSHIP_SCHEMA_ID" };

function same_value_p(left, right) {
  return (left === right);
}

function actor_equal_p(left, right) {
  return ((((left == null) && (right == null))) ? true : (((left == null) || (right == null))) ? false : (same_value_p(left.kind, right.kind) && same_value_p(left.id, right.id)));
}

function pending_offer_equal_p(left, right) {
  return ((((left == null) && (right == null))) ? true : (((left == null) || (right == null))) ? false : (same_value_p(left.id, right.id) && actor_equal_p(left.from, right.from) && actor_equal_p(left.to, right.to)));
}

function state_equal_p(left, right) {
  return (same_value_p(left.goal, right.goal) && actor_equal_p(left.owner, right.owner) && actor_equal_p(left.accountableParent, right.accountableParent) && pending_offer_equal_p(left.pendingOffer, right.pendingOffer));
}

function accepted_parent_p(before, after) {
  return actor_equal_p(after.accountableParent, before.owner);
}

function success(transition) {
  return [true, transition, null];
}

function reject(message) {
  return [false, null, message];
}

function validate_offer(transition, before, event, after) {
  const pending_before = before.pendingOffer;
  const pending_after = after.pendingOffer;
  const actor = event.actor;
  const recipient = event.to;
  return (((pending_before != null)) ? reject("offer requires no existing pending offer") : ((!actor_equal_p(actor, before.owner))) ? reject("only the current owner may offer work") : (actor_equal_p(actor, recipient)) ? reject("offer recipient must differ from the current owner") : ((!actor_equal_p(after.owner, before.owner))) ? reject("an offer must not move ownership") : ((!actor_equal_p(after.accountableParent, before.accountableParent))) ? reject("an offer must not change accountable parent") : ((pending_after == null)) ? reject("an offer must create a pending offer") : ((!same_value_p(pending_after.id, event.offerId))) ? reject("pending offer ID must match the offer event") : ((!actor_equal_p(pending_after.from, actor))) ? reject("pending offer source must be the current owner") : ((!actor_equal_p(pending_after.to, recipient))) ? reject("pending offer recipient must match the offer event") : success(transition));
}

function validate_acceptance(transition, before, event, after) {
  const pending = before.pendingOffer;
  const actor = event.actor;
  return (((pending == null)) ? reject("acceptance requires a pending offer") : ((!same_value_p(pending.id, event.offerId))) ? reject("acceptance must name the pending offer") : ((!actor_equal_p(pending.from, before.owner))) ? reject("pending offer source must still own the work") : ((!actor_equal_p(actor, pending.to))) ? reject("only the offered recipient may accept work") : ((!actor_equal_p(after.owner, actor))) ? reject("acceptance must move ownership to the accepting run") : ((!accepted_parent_p(before, after))) ? reject("acceptance must retain the previous owner as accountable parent") : ((after.pendingOffer != null)) ? reject("acceptance must clear the pending offer") : success(transition));
}

function validate_transfer(transition, before, event, after) {
  const actor = event.actor;
  const recipient = event.to;
  const acknowledged = (event.acknowledgedBy != null);
  return (((before.pendingOffer != null)) ? reject("direct transfer requires no pending offer") : ((!actor_equal_p(actor, before.owner))) ? reject("only the current owner may transfer work") : (actor_equal_p(actor, recipient)) ? reject("transfer recipient must differ from the current owner") : ((!acknowledged)) ? (state_equal_p(before, after) ? success(transition) : reject("unacknowledged transfer must not move ownership or accountability")) : ((!actor_equal_p(event.acknowledgedBy, recipient))) ? reject("transfer acknowledgement must come from the recipient") : ((!actor_equal_p(after.owner, recipient))) ? reject("acknowledged transfer must move ownership to the recipient") : ((!actor_equal_p(after.accountableParent, before.accountableParent))) ? reject("acknowledged transfer must preserve the existing accountable parent") : ((after.pendingOffer != null)) ? reject("acknowledged transfer must not create a pending offer") : success(transition));
}

function validate_refusal(transition, before, event, after) {
  const pending = before.pendingOffer;
  const actor = event.actor;
  return (((pending == null)) ? reject("refusal requires a pending offer") : ((!same_value_p(pending.id, event.offerId))) ? reject("refusal must name the pending offer") : ((!actor_equal_p(actor, pending.to))) ? reject("only the offered recipient may refuse work") : ((!actor_equal_p(after.owner, before.owner))) ? reject("refusal must leave owner and accountable parent unchanged") : ((!actor_equal_p(after.accountableParent, before.accountableParent))) ? reject("refusal must leave owner and accountable parent unchanged") : ((after.pendingOffer != null)) ? reject("refusal must clear only the pending offer") : success(transition));
}

function validate_escalation(transition, before, event, after) {
  return (((!actor_equal_p(event.actor, before.owner))) ? reject("only the current owner may escalate work") : ((before.accountableParent == null)) ? reject("escalation requires an accountable parent") : ((!actor_equal_p(event.to, before.accountableParent))) ? reject("escalation must return to the accountable parent") : ((!state_equal_p(before, after))) ? reject("escalation must not change owner, goal, accountability, or pending offer") : success(transition));
}

function validate_work_ownership_transition_result(transition) {
  const before = transition.before;
  const event = transition.event;
  const after = transition.after;
  const kind = event.kind;
  return (((!same_value_p(transition.version, WORK__OWNERSHIP__VERSION))) ? reject("work ownership transition version must be work-ownership-v1") : ((!same_value_p(before.goal, after.goal))) ? reject("work ownership events must not change the goal") : (same_value_p(kind, "offer")) ? validate_offer(transition, before, event, after) : (same_value_p(kind, "accept")) ? validate_acceptance(transition, before, event, after) : (same_value_p(kind, "transfer")) ? validate_transfer(transition, before, event, after) : (same_value_p(kind, "refuse")) ? validate_refusal(transition, before, event, after) : (same_value_p(kind, "escalate")) ? validate_escalation(transition, before, event, after) : reject("unknown work ownership event"));
}
export { validate_work_ownership_transition_result as "validate-work-ownership-transition-result" };
