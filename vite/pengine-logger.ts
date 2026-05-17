import * as readline from "node:readline";
import winston from "winston";
import type { Logger, LogLevel, LogOptions, LogErrorOptions, LogType } from "vite";

const LogLevels: Record<LogType | "silent", number> = {
  silent: 0,
  error: 1,
  warn: 2,
  info: 3,
};

export type PengineViteLoggerOptions = {
  /** When false, skip TTY clear-screen (should match Vite `clearScreen: false`). Default true. */
  allowClearScreen?: boolean;
};

/**
 * Vite `customLogger` backed by Winston: structured levels, timestamps on info+,
 * same dedupe / clear-screen behavior as Vite’s default logger.
 */
export function createPengineViteLogger(
  level: LogLevel = "info",
  opts: PengineViteLoggerOptions = {},
): Logger {
  const loggedErrors = new WeakSet<object>();
  const warnedMessages = new Set<string>();
  const thresh = LogLevels[level];
  const allowClearScreen = opts.allowClearScreen !== false;

  const winstonLog = winston.createLogger({
    silent: level === "silent",
    levels: winston.config.npm.levels,
    level: level === "silent" ? "error" : level,
    transports: [
      new winston.transports.Console({
        format: winston.format.combine(
          winston.format.colorize({ all: true }),
          winston.format.printf(({ message }) => `${message}`),
        ),
      }),
    ],
  });

  const canClearScreen = process.stdout.isTTY && !process.env.CI && allowClearScreen;
  const clear = canClearScreen
    ? () => {
        const repeatCount = process.stdout.rows - 2;
        const blank = repeatCount > 0 ? "\n".repeat(repeatCount) : "";
        // Match Vite’s clear-screen helper (scroll then home).
        if (blank) process.stdout.write(blank);
        readline.cursorTo(process.stdout, 0, 0);
        readline.clearScreenDown(process.stdout);
      }
    : () => {};

  const timeFmt = new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "numeric",
    second: "numeric",
  });

  let lastType: LogType | undefined;
  let lastMsg: string | undefined;
  let sameCount = 0;

  function formatLine(type: LogType, msg: string, opts: LogOptions = {}): string {
    const env = opts.environment ? `${opts.environment} ` : "";
    const lvl = type.toUpperCase();
    if (opts.timestamp) {
      return `${timeFmt.format(new Date())} (pengine:dev) [${lvl}] ${env}${msg}`;
    }
    return `(pengine:dev) [${lvl}] ${env}${msg}`;
  }

  function output(type: LogType, msg: string, opts: LogOptions = {}) {
    if (thresh < LogLevels[type]) {
      return;
    }
    if ("error" in opts && opts.error) {
      loggedErrors.add(opts.error as object);
    }
    const line = formatLine(type, msg, opts);
    const winstonMethod = type === "info" ? "info" : type;

    if (canClearScreen && type === lastType && msg === lastMsg) {
      sameCount += 1;
      clear();
      winstonLog.log(winstonMethod, `${line} (x${sameCount + 1})`);
    } else {
      sameCount = 0;
      lastMsg = msg;
      lastType = type;
      if (opts.clear) {
        clear();
      }
      winstonLog.log(winstonMethod, line);
    }
  }

  const logger: Logger = {
    hasWarned: false,
    info(msg, opts) {
      output("info", msg, opts ?? {});
    },
    warn(msg, opts) {
      logger.hasWarned = true;
      output("warn", msg, opts ?? {});
    },
    warnOnce(msg, opts) {
      if (warnedMessages.has(msg)) {
        return;
      }
      logger.hasWarned = true;
      output("warn", msg, opts ?? {});
      warnedMessages.add(msg);
    },
    error(msg, opts?: LogErrorOptions) {
      logger.hasWarned = true;
      output("error", msg, opts ?? {});
    },
    clearScreen(type: LogType) {
      if (thresh >= LogLevels[type]) {
        clear();
      }
    },
    hasErrorLogged(error) {
      return loggedErrors.has(error);
    },
  };

  return logger;
}
