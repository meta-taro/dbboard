// Desktop app: no server, so render entirely on the client and prerender the
// shell to static files the webview can load off disk.
export const ssr = false;
export const prerender = true;
