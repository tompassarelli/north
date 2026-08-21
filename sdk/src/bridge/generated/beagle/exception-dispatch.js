export const default_catch = Symbol("beagle/default-catch");

export function catch_dispatch(error, exceptionTypes) {
  for (let index = 0; index < exceptionTypes.length; index += 1) {
    const exceptionType = exceptionTypes[index];
    if (exceptionType === default_catch || error instanceof exceptionType) {
      return index;
    }
  }
  throw error;
}
