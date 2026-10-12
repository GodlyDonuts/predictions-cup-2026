param(
 [ValidateSet('Me','Send','Read')][string]$Action = 'Me',
 [string]$ChatId,
 [string]$Text,
 [long]$Offset = 0,
 [int]$TopicId = 0,
 [string]$CredentialTarget = 'Codex/Telegram/RhaegalCodex_bot'
)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
if ($Action -eq 'Send' -and (-not $ChatId -or -not $Text)) { throw 'Send requires -ChatId and -Text.' }
if ($Text.Length -gt 4096) { throw 'Keep messages under 4096 characters.' }
if (-not ('RhaegalTelegramCredential' -as [type])) {
 Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class RhaegalTelegramCredential {
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
 public struct Credential {
  public uint Flags; public uint Type; public string TargetName; public string Comment;
  public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
  public uint CredentialBlobSize; public IntPtr CredentialBlob; public uint Persist;
  public uint AttributeCount; public IntPtr Attributes; public string TargetAlias; public string UserName;
 }
 [DllImport("advapi32.dll",EntryPoint="CredReadW",CharSet=CharSet.Unicode,SetLastError=true)]
 public static extern bool Read(string target,uint type,uint flags,out IntPtr credential);
 [DllImport("advapi32.dll")] public static extern void CredFree(IntPtr credential);
}
'@
}
$credentialPointer = [IntPtr]::Zero
if (-not [RhaegalTelegramCredential]::Read($CredentialTarget,1,0,[ref]$credentialPointer)) {
 throw 'Rhaegal bot credential not found in Windows Credential Manager.'
}
try {
 $credential = [Runtime.InteropServices.Marshal]::PtrToStructure($credentialPointer,[type][RhaegalTelegramCredential+Credential])
 $botToken = [Runtime.InteropServices.Marshal]::PtrToStringUni($credential.CredentialBlob,[int]($credential.CredentialBlobSize/2))
} finally { [RhaegalTelegramCredential]::CredFree($credentialPointer) }
try {
 $method = switch ($Action) { Me {'getMe'} Send {'sendMessage'} Read {'getUpdates'} }
 $body = switch ($Action) {
  Me { @{} }
  Send {
   $payload = @{chat_id=$ChatId;text=$Text}
   if ($TopicId -gt 0) { $payload.message_thread_id=$TopicId }
   $payload
  }
  Read { @{offset=$Offset;limit=10;timeout=0;allowed_updates=@('message','channel_post','edited_message','edited_channel_post')} }
 }
 try {
  $response = Invoke-RestMethod -Method Post -Uri ("https://api.telegram.org/bot"+$botToken+"/"+$method) -ContentType 'application/json; charset=utf-8' -Body ($body|ConvertTo-Json -Compress) -TimeoutSec 30
 } catch { throw 'Telegram request failed. Check access, chat ID, webhook/poller conflicts, and bot-to-bot settings. Token-bearing errors suppressed; do not blindly retry a send.' }
 if (-not $response.ok) { throw 'Telegram rejected request.' }
 switch ($Action) {
  Me { @{ok=$true;username=$response.result.username;id=$response.result.id} | ConvertTo-Json -Compress }
  Send { @{ok=$true;chat_id=$response.result.chat.id;message_id=$response.result.message_id} | ConvertTo-Json -Compress }
  Read {
   $updates = @($response.result)
   $messages = @($updates | ForEach-Object {
    $message = if ($_.message) {$_.message} elseif ($_.channel_post) {$_.channel_post} elseif ($_.edited_message) {$_.edited_message} else {$_.edited_channel_post}
    $messageText = [string]$message.text
    @{update_id=$_.update_id;chat_id=$message.chat.id;topic_id=$message.message_thread_id;from=$message.from.username;text=$messageText.Substring(0,[Math]::Min(1200,$messageText.Length));truncated=($messageText.Length -gt 1200)}
   })
   $nextOffset = if ($updates.Count) { [long]($updates[-1].update_id)+1 } else { $Offset }
   @{status=$(if($updates.Count){'NEW_MESSAGES'}else{'NO_NEW_MESSAGES'});next_offset=$nextOffset;messages=$messages} | ConvertTo-Json -Depth 6 -Compress
  }
 }
} finally { $botToken=$null }
