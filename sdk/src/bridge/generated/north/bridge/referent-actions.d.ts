declare const referentActionArgv: (...args: any[]) => any;
declare const referentActionRequest: (...args: any[]) => any;
declare const runReferentAction: (...args: any[]) => any;
declare const semanticActionResultText: (...args: any[]) => any;
declare const validateCommittedReadback: (...args: any[]) => any;
declare const validateSemanticCatalog: (...args: any[]) => any;

export {
  referentActionArgv as "referent-action-argv!",
  referentActionRequest as "referent-action-request!",
  runReferentAction as "run-referent-action!",
  semanticActionResultText as "semantic-action-result-text!",
  validateCommittedReadback as "validate-committed-readback!",
  validateSemanticCatalog as "validate-semantic-catalog!",
};
