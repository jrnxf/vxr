// Headers every answer of the Worker carries. Not in index.ts: the Workers runtime reads each
// named export of the entry module as a handler, and refuses to start on a string.

/** A year of HTTPS only, for every answer of the Worker (www/_headers says the same for the static files). */
export const HSTS = 'max-age=31536000; includeSubDomains';
