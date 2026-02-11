# comment no Tabs, please

describe-yourself = Du bist ein hilfreicher Assistent.
  Benutzernachricht: {$p1}
  Sage deinen Namen und antworte dann!

which-task-for-you = Für welche dieser Aufgaben bist du geeignet?

three-qwestions = Ich brauche deine Hilfe bei drei Arten von Aufgaben!
  1. Verstehen, was auf dem Bild zu sehen ist.
  2. Arbeiten mit Werkzeugen.
  3. Nachdenken.

object-words = bauen konstruieren objekt erstellen machen
document-words = bild bildaufnahme video bericht dokument datei
description-words = beschreiben modifikation veränderung
comparison-words = vergleichen unterscheiden erkennen aktualisieren ändern
last-words = letzte vorherige jüngste
new-words = neu neueste
all-words = alle jede gesamte vollständige
period-words = tag woche monat quartal jahr
amount_num = 1 2 3 4 5 6 7 8 9 10
amount_text = eins zwei drei vier fünf sechs sieben acht neun zehn

# -------------------------------

# Progress Messages
progress-analyzing = Analysiere Ihre Anfrage...
progress-context-validation = Validiere Kontext...
progress-executing-worker = Führe {$worker_type} aus...
progress-formatting = Formatiere Antwort...

# Context Request Messages
context-request-object-id = Welches Gebäude oder welche Baustelle möchten Sie bearbeiten?
context-request-current-report = Welchen Fotobericht möchten Sie analysieren?
context-request-previous-report = Mit welchem vorherigen Bericht möchten Sie vergleichen?
context-request-clarification = Ich bin mir nicht sicher, was Sie meinen. Könnten Sie das bitte klarstellen?

# UI Messages
status-not-set = NICHT GESETZT
status-set = {$value}
no-conversation-history = Keine vorherige Konversation
no-worker-results = Noch keine Worker ausgeführt
worker-result-summary = {$worker_type}: {$status} ({$execution_time}ms)

# Error Messages
error-serialization = Serialisierungsfehler: {$error}
error-agent = Agentenfehler: {$error}
error-classification = Klassifizierungsergebnis konnte nicht geparst werden: {$error}
error-classification-fallback = Klassifizierungsergebnis konnte nicht geparst werden: {$error}
error-unknown-decision = Unbekannter Entscheidungstyp: {$decision_type}
error-missing-field = Fehlendes Feld {$field}
error-unknown-worker = Unbekannter Worker-Typ
error-unknown-context-field = Unbekanntes Kontextfeld
error-unknown-decision-type = Unbekannter Entscheidungstyp: {$decision_type}
error-empty-report-id = report_id darf nicht leer sein

# Orchestrator Messages
orchestrator-cannot-process = Cannot process this request

# Response Formatter Messages
error-comparison-parse = Failed to parse comparison: {$error}

# General Messages
analyzing-query = Analysiere Anfrage
fetching-data = Lade Daten
processing-results = Verarbeite Ergebnisse
