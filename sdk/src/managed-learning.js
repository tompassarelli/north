import { conj_value as $$bc$conj_value, distinct_equivV as $$bc$distinct_equiv, str as $$bc$str } from './bridge/generated/beagle/core.js';
import { admit_host_object as $$bh$admit_host_object, aset as $$bh$aset, js_obj as $$bh$js_obj } from './bridge/generated/beagle/host.js';
import { catch_dispatch as $$bd$catch_dispatch } from './bridge/generated/beagle/exception-dispatch.js';

const learning_regime_module = require("./learning-regime");

const assignLearningEpisode = learning_regime_module.assignLearningEpisode;

const loadLearningPolicy = learning_regime_module.loadLearningPolicy;

const routing_metadata_module = require("./routing-metadata");

const CAPABILITY_FLOORS = routing_metadata_module.CAPABILITY_FLOORS;

const SERVICE_CLASSES = routing_metadata_module.SERVICE_CLASSES;

const REASONING_LEVELS = routing_metadata_module.REASONING_LEVELS;

const provider_neutral_route_module = require("./provider-neutral-route");

const requireProviderNeutralRoute = provider_neutral_route_module.requireProviderNeutralRoute;

const composition_receipt_module = require("./composition-receipt");

const sha256Manifest = composition_receipt_module.sha256Manifest;

const ROUTE_AXES = ["capabilityFloor", "serviceClass", "reasoning"];

function learningRiskFromAssessment(...$beagle$args) {
  if (arguments.length === 0) {
    return undefined;
  }
  if (arguments.length === 1) {
    const assessment = $beagle$args[0];
    return (((assessment == null)) ? undefined : ((assessment.signals.errorExposure === "contained-reversible")) ? "p1" : ((assessment.signals.errorExposure === "material-recoverable")) ? "p2" : "p3");
  }
  throw new Error('No matching arity: ' + $beagle$args.length);
}

function supported_route_p(capability_floor, service_class, reasoning) {
  return (() => { try {
    requireProviderNeutralRoute(capability_floor, service_class, reasoning);
  return true;
  } catch (_catch_0) {
    switch ($$bd$catch_dispatch(_catch_0, [Error])) {
      case 0: {
        const __error = _catch_0;
        return false;
        break;
      }
    }
  } })();
}

function eligible_capability_floors(request) {
  return CAPABILITY_FLOORS.filter((capability_floor) => supported_route_p(capability_floor, request.serviceClass, request.reasoning));
}

function eligible_service_classes(request) {
  return SERVICE_CLASSES.filter((service_class) => supported_route_p(request.capabilityFloor, service_class, request.reasoning));
}

function eligible_reasoning_levels(request) {
  return REASONING_LEVELS.filter((reasoning) => ((!((reasoning === "max") && (!(request.reasoning === "max")))) && supported_route_p(request.capabilityFloor, request.serviceClass, reasoning)));
}

function route_arms(request) {
  return $$bh$js_obj("capabilityFloor", eligible_capability_floors(request), "serviceClass", eligible_service_classes(request), "reasoning", eligible_reasoning_levels(request));
}

function pinned_axes(assessment, pin) {
  const axes = [];
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? ((_logical) => (_logical !== false && _logical != null ? (!(assessment.exception.code === "calibration-experiment")) : _logical))(assessment.exception) : _logical))(assessment))) {
    axes.push("capabilityFloor", "serviceClass", "reasoning");
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? pin.pins.some((entry) => (entry.kind === "model")) : _logical))(pin))) {
    axes.push("capabilityFloor", "serviceClass");
  }
  return Array.from($$bc$distinct_equiv(axes));
}

function assigned_composition(request, field, assignment) {
  const composition = request.composition;
  if ((composition.kind === "template")) {
    const assigned = Object.assign($$bh$js_obj(), composition);
    const overrides = Array.from($$bc$distinct_equiv($$bc$conj_value(composition.overrides, field)));
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(assigned, "overrides", overrides);
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(assigned, "overrideReason", $$bc$str("learning assignment ", assignment.manifestSha256.slice(0, 16)));
    return assigned;
  } else {
    return composition;
  }
}

function assigned_routing_request(request, field, assignment) {
  const assigned = Object.assign($$bh$js_obj(), request);
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(assigned, field, assignment.armId);
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(assigned, "composition", assigned_composition(request, field, assignment));
  return assigned;
}

function assigned_assessment(assessment, request, assignment) {
  const assigned = Object.assign($$bh$js_obj(), assessment);
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(assigned, "selected", $$bh$js_obj("capabilityFloor", request.capabilityFloor, "reasoning", request.reasoning));
  (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(assigned, "exception", $$bh$js_obj("code", "calibration-experiment", "detail", $$bc$str("deterministic learning assignment ", assignment.manifestSha256.slice(0, 16))));
  if (((request.reasoning === "max") || (assessment.derived.minimumReasoning === "max"))) {
    if ((!((_truthy) => _truthy !== false && _truthy != null)(assigned.exceptionalDeliberation))) {
      (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(assigned, "exceptionalDeliberation", "deterministic learning evaluation requires the admitted maximum-deliberation arm");
    }
  } else {
    Reflect.deleteProperty(assigned, "exceptionalDeliberation");
  }
  return assigned;
}

function routing_assignment_p(assignment) {
  return ((assignment.arm === "explore") && ROUTE_AXES.includes(assignment.axis));
}

function apply_route_assignment(request, assessment, assignment) {
  if ((!routing_assignment_p(assignment))) {
    return (((_truthy) => _truthy !== false && _truthy != null)(assessment) ? $$bh$js_obj("assignment", assignment, "routingMetadata", request, "routingAssessment", assessment) : $$bh$js_obj("assignment", assignment, "routingMetadata", request));
  } else {
    const field = assignment.axis;
    const assigned = assigned_routing_request(request, field, assignment);
    return (((_truthy) => _truthy !== false && _truthy != null)(assessment) ? $$bh$js_obj("assignment", assignment, "routingMetadata", assigned, "routingAssessment", assigned_assessment(assessment, assigned, assignment)) : $$bh$js_obj("assignment", assignment, "routingMetadata", assigned));
  }
}

function decideManagedLearning(input) {
  const request = input.routingMetadata;
  const assessment = ((_logical) => (_logical !== false && _logical != null ? _logical : null))(input.routingAssessment);
  const arms = route_arms(request);
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (input.promptArms.length > 0) : _logical))(input.promptArms))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(arms, "prompt", input.promptArms);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (input.authoringArms.length > 0) : _logical))(input.authoringArms))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(arms, "authoring", input.authoringArms);
  }
  if (((_truthy) => _truthy !== false && _truthy != null)(((_logical) => (_logical !== false && _logical != null ? (input.historyArms.length > 0) : _logical))(input.historyArms))) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(arms, "history", input.historyArms);
  }
  const assignment_input = $$bh$js_obj("episodeId", input.episodeId, "taskSignatureSha256", sha256Manifest(input.taskSignature), "taskSignatureCoverage", input.taskSignatureCoverage, "baseline", $$bh$js_obj("capabilityFloor", request.capabilityFloor, "serviceClass", request.serviceClass, "reasoning", request.reasoning, "prompt", "managed-baseline", "authoring", "managed-capability-contract", "history", "git"), "hardFloor", $$bh$js_obj("capabilityFloor", (((_truthy) => _truthy !== false && _truthy != null)(assessment) ? assessment.derived.minimumCapabilityFloor : request.capabilityFloor), "reasoning", (((_truthy) => _truthy !== false && _truthy != null)(assessment) ? assessment.derived.minimumReasoning : request.reasoning)), "eligibleArms", arms, "pinnedAxes", pinned_axes(assessment, ((_logical) => (_logical !== false && _logical != null ? _logical : null))(input.pinEvidence)));
  const risk = learningRiskFromAssessment(assessment);
  if (((_truthy) => _truthy !== false && _truthy != null)(risk)) {
    (($beagle$host$arg$0, $beagle$host$arg$1, $beagle$host$arg$2) => $$bh$aset($$bh$admit_host_object($beagle$host$arg$0), $beagle$host$arg$1, $beagle$host$arg$2))(assignment_input, "risk", risk);
  }
  const assignment = assignLearningEpisode(loadLearningPolicy(), assignment_input);
  return apply_route_assignment(request, assessment, assignment);
}

export { decideManagedLearning as "decideManagedLearning" };
export { learningRiskFromAssessment as "learningRiskFromAssessment" };
