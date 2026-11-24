export function capturedBaseURL(envVar = 'BROWSER_PORT'): string {
	const port = process.env[envVar];
	if (!port || !/^\d+$/.test(port)) {
		throw new Error(`Browser server port was not captured in ${envVar}`);
	}
	return `http://127.0.0.1:${port}`;
}
