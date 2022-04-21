const sodium = require('sodium-native');
const bunyan = require('bunyan');
const crypto = require('crypto');
const os = require('os');
const { PerformanceObserver, performance } = require('perf_hooks');

const { StateDB } = require('../main.js');

const logger = bunyan.createLogger({
    name: 'benchmark',
    streams: [
        {
            level: 'info',
            stream: process.stdout            // log INFO and above to stdout
        },
        {
            level: 'info',
            path: __dirname + '/benchmark.log'  // log ERROR and above to a file
        }
    ]
});

const getRandomBytes = length => {
    const nonce = Buffer.alloc(length);
    sodium.randombytes_buf(nonce);
    return nonce;
}

const hash = data => {
    const dataHash = crypto.createHash('sha256');
    dataHash.update(data);

    return dataHash.digest();
};

const count = 10000;

(async () => {
    let root = Buffer.alloc(0);
    let db = new StateDB('./.tmp-state');
    logger.info({ cpus: os.cpus(), arch: os.arch(), mem: os.totalmem(), ver: os.version() }, "Starting benchmark");
    const obs = new PerformanceObserver((items) => {
        for (const entry of items.getEntries()) {
            logger.info({ duration: entry.duration, name: entry.name });
        }
        // performance.clearMarks();
    });
    obs.observe({ type: 'measure' });

    let totalData = 0;
    for (let i = 0; i < 10000; i++) {
        logger.info(`Executing ${i + 1} with root ${root.toString('hex')}`);
        performance.mark('s-start');
        for (let j = 0; j < count; j++) {
            db.set(Buffer.concat([Buffer.from([0,0,0,0,0,0]), getRandomBytes(20)]), getRandomBytes(100));
        }
        performance.mark('s-end');
        performance.mark('c-start');
        root = await db.commit(root);
        performance.mark('c-end');

        performance.measure(`Setting ${i}`, 's-start', 's-end');
        performance.measure(`Commit ${i}`, 'c-start', 'c-end');
        totalData += count;
        logger.info({ totalData }, `Completed`);
    }

    db.close();
})()