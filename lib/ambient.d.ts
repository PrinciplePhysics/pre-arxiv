// Ambient type declarations so `npx tsc --noEmit` works against this codebase
// without requiring `@types/node` to be installed. We declare the bare
// minimum surface PreXiv touches — not a full Node typing.
//
// If/when @types/node is ever added to devDependencies, this file can be
// deleted (or its declarations narrowed to `// @ts-expect-error redundant`
// stubs that get caught by the linter).

declare var __dirname: string;
declare var __filename: string;
declare var process: {
  env: { [key: string]: string | undefined };
  argv: string[];
  cwd(): string;
  exit(code?: number): never;
  uptime(): number;
  hrtime: { bigint(): bigint };
  version: string;
  stdout: { write(s: string): boolean };
  stderr: { write(s: string): boolean };
  on(event: string, listener: (...args: any[]) => void): void;
};

declare var require: {
  (id: string): any;
  main: { module?: any } | undefined;
  resolve(id: string): string;
};
declare var module: { exports: any };
declare var exports: any;

interface Buffer extends Uint8Array {
  toString(encoding?: string): string;
  writeBigUInt64BE(value: bigint, offset?: number): number;
  length: number;
}
declare var Buffer: {
  alloc(size: number): Buffer;
  from(data: any, encoding?: string): Buffer;
  isBuffer(obj: any): boolean;
};

declare module 'path';
declare module 'fs';
declare module 'dns';
declare module 'net';
declare module 'crypto';
declare module 'http';
declare module 'https';
declare module 'url';
declare module 'os';
declare module 'util';
declare module 'stream';
declare module 'querystring';

declare module 'better-sqlite3';
declare module 'connect-sqlite3';
declare module 'express-rate-limit';
declare module 'express-session';
declare module 'helmet';
declare module 'multer';
declare module 'bcryptjs';
declare module 'sanitize-html';
declare module 'marked';
declare module 'pdf-parse';
declare module 'ejs';

// Express has ambient @types nearby (via npm tree dependency) — but if it's
// missing, fall back to a permissive surface that satisfies the JSDoc
// `import('express').Application` / `Request` / `Response` / `NextFunction` /
// `RequestHandler` references in routes/*.js.
declare module 'express' {
  export type Request = any;
  export type Response = any;
  export type NextFunction = any;
  export type RequestHandler = any;
  export type Application = any;
  // The runtime export is both callable (`express()` returns an app) and has
  // method properties (`express.static`, `express.urlencoded`, …).
  interface ExpressNamespace {
    (): any;
    static: any;
    urlencoded: any;
    json: any;
    Router: any;
    [k: string]: any;
  }
  const e: ExpressNamespace;
  export = e;
}

declare module 'multer' {
  export type Multer = any;
  const m: any;
  export default m;
}

// Buffer-related globals used in lib/totp.js
type BufferEncoding =
  | 'ascii' | 'utf8' | 'utf-8' | 'utf16le' | 'ucs2' | 'ucs-2'
  | 'base64' | 'base64url' | 'latin1' | 'binary' | 'hex';

declare var setImmediate: (cb: (...args: any[]) => void, ...args: any[]) => any;
declare var clearImmediate: (handle: any) => void;
// In Node, setTimeout returns a Timeout object with .unref() / .ref(); in
// browsers it's a number. Override the DOM lib's number-returning shape so
// `.unref()` typechecks.
interface Timeout {
  ref(): Timeout;
  unref(): Timeout;
}
declare function setTimeout(cb: (...args: any[]) => void, ms: number, ...args: any[]): Timeout;
declare function clearTimeout(handle: Timeout | number | undefined): void;
