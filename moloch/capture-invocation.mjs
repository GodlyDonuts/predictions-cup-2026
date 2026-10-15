import path from 'node:path';
export function captureInvocation(root,{scopedFile,recoveryWindowsFile,capture,cycles,exitPriorityBooks,dnsReadRecovery,realtime,universePath},platform=process.platform){
 if(platform==='win32')return{command:'pwsh',args:['-NoProfile','-File',path.join(root,'moloch/capture-sig.ps1'),'-ScreenPairsFile',scopedFile,'-RecoveryWindowsFile',recoveryWindowsFile,'-OutputDirectory',capture,'-Cycles',String(cycles),'-HumanAuthorizedSharedReadCapture',...(exitPriorityBooks?['-ExitPriorityBooks']:[]),...(dnsReadRecovery?['-AllowOneDnsReadRecovery']:[]),...(realtime?['-RealtimeCapture']:[]),...(universePath?['-UniversePairsFile',universePath]:[])]};
 if(platform!=='linux')throw Error('MOLOCH_CAPTURE_PLATFORM_UNSUPPORTED');
 return{command:process.execPath,args:[path.join(root,'moloch/vps/capture.mjs'),'--pairs',scopedFile,'--recovery',recoveryWindowsFile,'--output',capture,'--cycles',String(cycles),...(exitPriorityBooks?['--priority']:[]),...(dnsReadRecovery?['--dns-recovery']:[]),...(realtime?['--realtime']:[]),...(universePath?['--universe',universePath]:[])]};
}
