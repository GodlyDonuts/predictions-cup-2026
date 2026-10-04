import {parentPort,workerData} from 'node:worker_threads';
import {replayPortfolioWorkerJob} from './portfolio-bank.mjs';
if(!parentPort)throw Error('Offline replay worker requires its verified parent process');
const paths=replayPortfolioWorkerJob(workerData,progress=>parentPort.postMessage({kind:'progress',progress}));
parentPort.postMessage({kind:'result',entries:[...paths]});
parentPort.close();
