export class ExceptionInfo extends Error {
  constructor(message, data, cause = null) {
    super(message);
    this.data = data;
    this.cause = cause;
  }
}

export function ex_info(message, data, cause = null) {
  return new ExceptionInfo(message, data, cause);
}

export function ex_data(error) {
  return error instanceof ExceptionInfo ? error.data : null;
}

export function ex_message(error) {
  return error instanceof Error ? error.message : null;
}

export function ex_cause(error) {
  return error instanceof ExceptionInfo ? error.cause : null;
}
