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

# locales/de/messages.ftl
# ============================================================
# German localisation for cx58-agent
# ============================================================

# ------------------------------------------------------------
# Progress messages
# ------------------------------------------------------------

progress-analyzing = Anfrage wird analysiert…
progress-context-validation = Kontext wird geprüft…
progress-formatting = Antwort wird aufbereitet…

# ------------------------------------------------------------
# Info / status messages
# ------------------------------------------------------------

info-no-documents-found = Es wurden keine Dokumente gefunden, die den Kriterien entsprechen.
info-out-of-scope = Diese Anfrage liegt außerhalb des Bereichs des Baustellenüberwachungssystems.

# ------------------------------------------------------------
# AgentError messages
# ------------------------------------------------------------
error-cancelled = Die Anfrage wurde abgebrochen.

error-missing-object-id =
    Es wurde keine Objekt-ID übergeben.
    Bitte wählen Sie ein Objekt aus und versuchen Sie es erneut.

error-invalid-uuid =
    Der Wert „{ $raw }" ist keine gültige ID.
    Bitte versuchen Sie es erneut oder wenden Sie sich an den Support.

error-object-not-found =
    Das Objekt „{ $id }" wurde nicht gefunden.
    Bitte prüfen Sie die Objekt-ID und versuchen Sie es erneut.

error-no-documents-found =
    Es wurden keine Dokumente gefunden, die Ihren Kriterien entsprechen.
    Versuchen Sie einen anderen Zeitraum oder ein anderes Objekt.

error-llm-json-parse =
    Das KI-Modell hat eine unerwartete Antwort zurückgegeben, die nicht verarbeitet werden konnte.
    Details: { $detail }

error-template-render =
    Die Vorlage „{ $template }" konnte nicht gerendert werden.
    Bitte wenden Sie sich an den Support, wenn dieses Problem weiterhin auftritt.

error-localization-key-missing =
    Fehlender Übersetzungsschlüssel: { $key }.
    Bitte melden Sie dies dem Support.

error-date-parse =
    Das Datum „{ $raw }" konnte nicht verarbeitet werden.
    Bitte verwenden Sie das Format TT.MM.JJJJ HH:MM:SS.

error-storage =
    Ein Speicherfehler ist aufgetreten: { $detail }.
    Bitte versuchen Sie es später erneut.

error-insufficient-descriptions =
    Für einen Vergleich werden mindestens 2 Berichtsbeschreibungen benötigt, aber nur { $found } waren verfügbar.

error-internal =
    Ein interner Fehler ist aufgetreten: { $detail }
    Bitte versuchen Sie es erneut oder wenden Sie sich an den Support.
