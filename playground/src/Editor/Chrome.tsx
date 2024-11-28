import { useCallback, useMemo, useRef, useState } from "react";
import { default as Editor, Source } from "./Editor";
import initSqruff from "../pkg";
import Header from "./Header";

export default function Chrome() {
  const initPromise = useRef<null | Promise<void>>(null);
  const [sqlSource, setSqlSource] = useState<null | string>(null);
  const [settings, setSettings] = useState<null | string>(null);

  if (initPromise.current == null) {
    initPromise.current = startPlayground()
      .then(({ sourceCode, settings }) => {
        setSqlSource(sourceCode);
        setSettings(settings);
      })
      .catch((error) => {
        console.error("Failed to initialize playground.", error);
      });
  }

  const handleSourceChanged = useCallback((source: string) => {
    setSqlSource(source);
  }, []);

  const handleSettingsChanged = useCallback((settings: string) => {
    setSettings(settings);
  }, []);

  const handleNewIssue = useCallback(() => {
    if (settings == null || sqlSource == null) {
      return;
    }

    const bugReport = `
### What Happened

### Expected Behaviour

### How to reproduce

\`\`\`sql
${sqlSource}
\`\`\`

### Configuration
\`\`\`ini
${settings}
\`\`\`
`;
    const github = new URL("https://github.com/quarylabs/sqruff/issues/new");
    github.searchParams.set("body", bugReport);

    const newWindow = window.open(github, "_blank");
    if (newWindow) {
      newWindow.focus();
    }
  }, [sqlSource, settings]);

  const source: Source | null = useMemo(() => {
    if (sqlSource == null || settings == null) {
      return null;
    }

    return { sqlSource: sqlSource, settingsSource: settings };
  }, [settings, sqlSource]);

  return (
    <main className="flex flex-col h-full">
      <Header onNewIssue={handleNewIssue} />
      <div className="flex flex-grow">
        {source != null && (
          <Editor
            source={source}
            onSettingsChanged={handleSettingsChanged}
            onSourceChanged={handleSourceChanged}
          />
        )}
      </div>
    </main>
  );
}

async function startPlayground(): Promise<{
  sourceCode: string;
  settings: string;
}> {
  await initSqruff();

  const [settingsSource, sqlSource] = [
    "[sqruff]\ndialect = ansi\nrules = core\n",
    "SELECT name from USERS",
  ];

  return {
    sourceCode: sqlSource,
    settings: settingsSource,
  };
}
