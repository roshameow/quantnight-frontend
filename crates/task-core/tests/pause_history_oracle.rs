use quantnight_task_core::pause;
#[test]
fn original_python_history_replay_oracle() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(ORACLE).unwrap();
    for case in cases {
        let row = bson::Bson::try_from(case["task"].clone())
            .unwrap()
            .as_document()
            .unwrap()
            .clone();
        let actual = pause::never_started_history(&row);
        if let Some(error) = case["expected"].get("error") {
            assert_eq!(
                actual,
                Err(error.as_str().unwrap().to_string()),
                "{}",
                case["label"]
            );
        } else {
            assert_eq!(
                actual,
                Ok(case["expected"]["value"].as_bool().unwrap()),
                "{}",
                case["label"]
            );
        }
    }
}
const ORACLE: &str = r###"[
  {
    "label": "pending",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "pending",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        }
      ]
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "ready",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "failed",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "generation_failed",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "generation_failed",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "prepared",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "pending",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "preparing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "pending",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "prepared",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "remote",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": "2024-01-01T00:00:08+00:00"
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "retry",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "launchAttemptId=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptId": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "launchAttemptId=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptId": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptId=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptId": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptId=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptId": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptId=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptId": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptId={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptId": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptId='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptId": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptAt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptAt": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "launchAttemptAt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptAt": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptAt=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptAt": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptAt=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptAt": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptAt=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptAt": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptAt={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptAt": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttemptAt='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttemptAt": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startedAt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startedAt": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "startedAt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startedAt": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startedAt=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startedAt": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startedAt=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startedAt": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startedAt=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startedAt": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startedAt={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startedAt": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startedAt='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startedAt": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "completedAt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "completedAt": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "completedAt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "completedAt": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "completedAt=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "completedAt": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "completedAt=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "completedAt": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "completedAt=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "completedAt": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "completedAt={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "completedAt": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "completedAt='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "completedAt": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "pausedAt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "pausedAt": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "pausedAt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "pausedAt": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "pausedAt=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "pausedAt": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "pausedAt=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "pausedAt": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "pausedAt=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "pausedAt": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "pausedAt={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "pausedAt": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "pausedAt='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "pausedAt": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumer_run_id=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumer_run_id": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "consumer_run_id=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumer_run_id": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumer_run_id=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumer_run_id": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumer_run_id=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumer_run_id": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumer_run_id=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumer_run_id": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumer_run_id={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumer_run_id": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumer_run_id='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumer_run_id": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "heartbeatAt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "heartbeatAt": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "heartbeatAt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "heartbeatAt": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "heartbeatAt=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "heartbeatAt": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "heartbeatAt=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "heartbeatAt": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "heartbeatAt=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "heartbeatAt": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "heartbeatAt={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "heartbeatAt": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "heartbeatAt='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "heartbeatAt": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startPreflight=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startPreflight": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "startPreflight=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startPreflight": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startPreflight=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startPreflight": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startPreflight=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startPreflight": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startPreflight=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startPreflight": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startPreflight={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startPreflight": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startPreflight='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startPreflight": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "stopConfirmedAt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "stopConfirmedAt": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "stopConfirmedAt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "stopConfirmedAt": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "stopConfirmedAt=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "stopConfirmedAt": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "stopConfirmedAt=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "stopConfirmedAt": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "stopConfirmedAt=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "stopConfirmedAt": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "stopConfirmedAt={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "stopConfirmedAt": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "stopConfirmedAt='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "stopConfirmedAt": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchHistory=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchHistory": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "launchHistory=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchHistory": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchHistory=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchHistory": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchHistory=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchHistory": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchHistory=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchHistory": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchHistory={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchHistory": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchHistory='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchHistory": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttempts=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttempts": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "launchAttempts=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttempts": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttempts=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttempts": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttempts=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttempts": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttempts=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttempts": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttempts={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttempts": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchAttempts='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchAttempts": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchReceipt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchReceipt": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "launchReceipt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchReceipt": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchReceipt=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchReceipt": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchReceipt=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchReceipt": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchReceipt=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchReceipt": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchReceipt={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchReceipt": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "launchReceipt='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "launchReceipt": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startReceipt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startReceipt": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "startReceipt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startReceipt": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startReceipt=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startReceipt": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startReceipt=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startReceipt": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startReceipt=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startReceipt": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startReceipt={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startReceipt": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "startReceipt='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "startReceipt": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumerPid=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumerPid": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "consumerPid=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumerPid": false
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumerPid=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumerPid": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumerPid=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumerPid": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumerPid=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumerPid": []
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumerPid={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumerPid": {}
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "consumerPid='past-launch'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "consumerPid": "past-launch"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "status='ready'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "status='paused'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "paused",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "status='running'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "running",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "status='generating'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "generating",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "status='generation_unknown'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "generation_unknown",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "status='syncing'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "syncing",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "status=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": [],
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'list'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "status={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": {},
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'dict'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "taskType='super'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "super",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "taskType='priority'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "priority",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "taskType=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": null,
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "taskType=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": [],
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'list'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "taskType={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": {},
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'dict'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "isRemote=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": null,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "isRemote=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": {
        "$numberInt": "0"
      },
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "isRemote=1",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": {
        "$numberInt": "1"
      },
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "isRemote='false'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": "false",
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "isRemote=True",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "targetRemote=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": null,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "targetRemote=0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": {
        "$numberInt": "0"
      },
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "targetRemote=1",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": {
        "$numberInt": "1"
      },
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "targetRemote='true'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": "true",
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "targetRemote=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": false,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "auth_profile=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "auth_profile=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": null,
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "auth_profile=7",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": {
        "$numberInt": "7"
      },
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "auth_profile=' '",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": " ",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "templatefile=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "templatefile=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": null,
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "templatefile=7",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": {
        "$numberInt": "7"
      },
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "templatefile=' '",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": " ",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "runId=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": null,
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "runId='wrong'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "wrong",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "runId='mission:fixture '",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture ",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": null,
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {},
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": [],
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "createdAt='2024-01-01T00:00:00'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "createdAt='2024-01-01T00:00:00Z'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00Z",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "createdAt='2024-01-01 00:00:00+00:00'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01 00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "createdAt='not-a-date'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "not-a-date",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "createdAt='2016-12-31T23:59:60+00:00'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2016-12-31T23:59:60+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "createdAt='2024-01-01T00:00:00z'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00z",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "createdAt='20240101T000000+0000'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "20240101T000000+0000",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "createdAt='2024-01-01T00:00+00:00'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "events=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": null,
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "events=[]",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "events={}",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": {},
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "syncedAt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedAt": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "syncedAt=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedAt": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "syncedAt='2024-01-01T00:00:08+00:00'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedAt": "2024-01-01T00:00:08+00:00"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "syncedDigest=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": null
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "syncedDigest=''",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": ""
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "syncedDigest='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.id=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": null,
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.id='wrong'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "wrong",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.id=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": false,
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.id=1",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": {
          "$numberInt": "1"
        },
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.id=37.0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": {
          "$numberDouble": "37.0"
        },
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.phase=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": null,
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.phase='wrong'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "wrong",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.phase=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": false,
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.phase=1",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": {
          "$numberInt": "1"
        },
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.phase=37.0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": {
          "$numberDouble": "37.0"
        },
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.startedAt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": null,
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.startedAt='wrong'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "wrong",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.startedAt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": false,
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.startedAt=1",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": {
          "$numberInt": "1"
        },
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.startedAt=37.0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": {
          "$numberDouble": "37.0"
        },
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.finishedAt=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": null,
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.finishedAt='wrong'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "wrong",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.finishedAt=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": false,
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.finishedAt=1",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": {
          "$numberInt": "1"
        },
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.finishedAt=37.0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": {
          "$numberDouble": "37.0"
        },
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.producerPid=None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": null
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.producerPid='wrong'",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": "wrong"
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.producerPid=False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": false
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.producerPid=1",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "1"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "generation.producerPid=37.0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberDouble": "37.0"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "missing_old_begin",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "unfinished_old_attempt",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "reused_old_token",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "wrong_finish_token",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "other"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "hidden_old_launch",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-3",
          "command": "start",
          "status": "running",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {}
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "begin_boolean_pid",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": true
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "begin_float_pid",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberDouble": "37.0"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "begin_outside_window",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:09+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "backwards_begin",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "future_event",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "9999-01-01T00:00:00+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "empty_first_id",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "bad_first_detail",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": []
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "watch:False:False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": false,
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "watch:True:False",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": false,
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": "2024-01-01T00:00:08+00:00"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "watch:False:True",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": true,
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "watch:True:True",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": true,
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": "2024-01-01T00:00:08+00:00"
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "watch:False:0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": {
              "$numberInt": "0"
            },
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "watch:True:0",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": {
              "$numberInt": "0"
            },
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": "2024-01-01T00:00:08+00:00"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "watch:False:1",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": {
              "$numberInt": "1"
            },
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "watch:True:1",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": {
              "$numberInt": "1"
            },
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": "2024-01-01T00:00:08+00:00"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "watch:False:None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": null,
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "watch:True:None",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        },
        {
          "id": "event-9",
          "command": "watch-attach",
          "status": "ok",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {
            "missionId": "0123456789abcdef01234567",
            "remote": null,
            "launchId": "legacy"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": "2024-01-01T00:00:08+00:00"
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "idempotent-pending",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "pending",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-9",
          "command": "create",
          "status": "idempotent",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {}
        }
      ]
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "idempotent-ready",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-9",
          "command": "create",
          "status": "idempotent",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {}
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "idempotent-generating",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-2",
          "command": "create",
          "status": "idempotent",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {}
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "idempotent-generation_failed",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "generation_failed",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "generation_failed",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-9",
          "command": "create",
          "status": "idempotent",
          "at": "2024-01-01T00:00:09+00:00",
          "detail": {}
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "generation_failed",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "0"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "fresh_final_token_cannot_erase_open_old_token",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-5",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:05+00:00",
          "detail": {
            "id": "gen-2",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:04+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-6",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:06+00:00",
          "detail": {
            "generationId": "gen-2"
          }
        }
      ],
      "generation": {
        "id": "gen-2",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:04+00:00",
        "finishedAt": "2024-01-01T00:00:06+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "bson-hash:status:Code",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": {
        "$code": "pending"
      },
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'Code'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "bson-hash:taskType:Code",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": {
        "$code": "pending"
      },
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'Code'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "bson-hash:status:Code",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": {
        "$code": "pending",
        "$scope": {
          "x": {
            "$numberInt": "1"
          }
        }
      },
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'Code'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "bson-hash:taskType:Code",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": {
        "$code": "pending",
        "$scope": {
          "x": {
            "$numberInt": "1"
          }
        }
      },
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'Code'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "bson-hash:status:Regex",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": {
        "$regularExpression": {
          "pattern": "ready",
          "options": ""
        }
      },
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'Regex'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "bson-hash:taskType:Regex",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": {
        "$regularExpression": {
          "pattern": "ready",
          "options": ""
        }
      },
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'Regex'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "bson-hash:status:Decimal128",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": {
        "$numberDecimal": "1"
      },
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'Decimal128'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "bson-hash:taskType:Decimal128",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": {
        "$numberDecimal": "1"
      },
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "error": "unhashable type: 'Decimal128'",
      "errorType": "TypeError"
    }
  },
  {
    "label": "string-instance:auth_profile:Code('fixture', None)",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": {
        "$code": "fixture"
      },
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "string-instance:auth_profile:Code('', None)",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": {
        "$code": ""
      },
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "string-instance:auth_profile:Code('fixture', {'scope': 1})",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": {
        "$code": "fixture",
        "$scope": {
          "scope": {
            "$numberInt": "1"
          }
        }
      },
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "string-instance:templatefile:Code('fixture', None)",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": {
        "$code": "fixture"
      },
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "string-instance:templatefile:Code('', None)",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": {
        "$code": ""
      },
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "string-instance:templatefile:Code('fixture', {'scope': 1})",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": {
        "$code": "fixture",
        "$scope": {
          "scope": {
            "$numberInt": "1"
          }
        }
      },
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "exact-python-int:begin",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberLong": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "exact-python-int:generation",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberLong": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "exact-python-int:both",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": false,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberLong": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberLong": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "synced-truthiness:Binary(b'', 0)",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": {
        "$binary": {
          "base64": "",
          "subType": "00"
        }
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "synced-truthiness:Binary(b'x', 0)",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": {
        "$binary": {
          "base64": "eA==",
          "subType": "00"
        }
      }
    },
    "expected": {
      "value": true
    }
  },
  {
    "label": "synced-truthiness:Code('', None)",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": {
        "$code": ""
      }
    },
    "expected": {
      "value": false
    }
  },
  {
    "label": "synced-truthiness:Code('present', None)",
    "task": {
      "_id": "0123456789abcdef01234567",
      "name": "fixture",
      "status": "ready",
      "taskType": "regular",
      "isRemote": true,
      "targetRemote": true,
      "auth_profile": "fixture-profile",
      "templatefile": "fixture-template",
      "runId": "mission:fixture",
      "createdAt": "2024-01-01T00:00:00+00:00",
      "events": [
        {
          "id": "event-0",
          "command": "create",
          "status": "pending",
          "at": "2024-01-01T00:00:00+00:00",
          "detail": {}
        },
        {
          "id": "event-2",
          "command": "generation-begin",
          "status": "generating",
          "at": "2024-01-01T00:00:02+00:00",
          "detail": {
            "id": "gen-1",
            "phase": "writing",
            "startedAt": "2024-01-01T00:00:01+00:00",
            "producerPid": {
              "$numberInt": "37"
            }
          }
        },
        {
          "id": "event-3",
          "command": "generation-finish",
          "status": "ready",
          "at": "2024-01-01T00:00:03+00:00",
          "detail": {
            "generationId": "gen-1"
          }
        },
        {
          "id": "event-7",
          "command": "sync-begin",
          "status": "syncing",
          "at": "2024-01-01T00:00:07+00:00",
          "detail": {}
        },
        {
          "id": "event-8",
          "command": "sync",
          "status": "ready",
          "at": "2024-01-01T00:00:08+00:00",
          "detail": {
            "total": {
              "$numberInt": "2"
            }
          }
        }
      ],
      "generation": {
        "id": "gen-1",
        "phase": "complete",
        "startedAt": "2024-01-01T00:00:01+00:00",
        "finishedAt": "2024-01-01T00:00:03+00:00",
        "producerPid": {
          "$numberInt": "37"
        }
      },
      "expectedCount": {
        "$numberInt": "2"
      },
      "syncedDigest": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "syncedAt": {
        "$code": "present"
      }
    },
    "expected": {
      "value": true
    }
  }
]"###;
